use super::*;
use serde_json::json;
use std::{
    fs,
    path::{MAIN_SEPARATOR_STR, Path},
    sync::mpsc,
    time::Duration,
};

/// 创建只含 captured Lease 的 fixture，证明列表不依赖 Desktop 或 Serena 状态。
fn lease(root: &Path, id: &str, generation: u64) -> WorkspaceLease {
    WorkspaceLease {
        workspace_id: id.into(),
        canonical_root: root.canonicalize().unwrap(),
        generation,
    }
}

/// 构造冻结 public 字段名的最小列表参数。
fn arguments(relative_path: &str) -> SourceArgs {
    SourceArgs {
        relative_path: Some(relative_path.into()),
        ..Default::default()
    }
}

/// 调用公开 handler，确保 DTO、budget 与 cancellation 都走真实入口。
async fn list_at(lease: &WorkspaceLease, arguments: SourceArgs) -> Result<Value, String> {
    list(lease, arguments, CancellationToken::new()).await
}

/// 将 JSON text 的两个稳定分组读成字符串数组，断言没有额外公开字段。
fn listing(output: &Value) -> Value {
    let listing: Value = serde_json::from_str(output["text"].as_str().unwrap()).unwrap();
    assert_eq!(listing.as_object().unwrap().len(), 2);
    assert!(listing["dirs"].is_array());
    assert!(listing["files"].is_array());
    listing
}

/// 当前平台的相对路径分隔符必须与既有 list_dir 文本契约一致。
fn path(parts: &[&str]) -> String {
    parts.join(MAIN_SEPARATOR_STR)
}

/// empty directory 必须以完整 JSON object 而非空文本返回。
#[tokio::test]
async fn source_list_dir_lists_an_empty_directory() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("empty")).unwrap();

    let output = list_at(&lease(&root, "workspace-a", 12), arguments("empty"))
        .await
        .unwrap();
    assert_eq!(
        output["workspace"],
        json!({"id":"workspace-a","generation":12})
    );
    assert_eq!(listing(&output), json!({"dirs":[],"files":[]}));
    assert_eq!(output["truncated"], false);
}

/// `.` 只能枚举各自 captured Lease 的 root，不能因进程目录或全局选择串到另一个 Workspace。
#[tokio::test]
async fn source_list_dir_uses_captured_lease_root_for_dot() {
    let directory = tempfile::tempdir().unwrap();
    let root_a = directory.path().join("a");
    let root_b = directory.path().join("b");
    fs::create_dir(&root_a).unwrap();
    fs::create_dir(&root_b).unwrap();
    fs::write(root_a.join("only-a.txt"), "a").unwrap();
    fs::write(root_b.join("only-b.txt"), "b").unwrap();
    let lease_a = lease(&root_a, "workspace-a", 12);
    let lease_b = lease(&root_b, "workspace-b", 31);

    let (a, b) = tokio::join!(
        list_at(&lease_a, arguments(".")),
        list_at(&lease_b, arguments(".")),
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a["workspace"], json!({"id":"workspace-a","generation":12}));
    assert_eq!(b["workspace"], json!({"id":"workspace-b","generation":31}));
    assert_eq!(listing(&a)["files"], json!(["only-a.txt"]));
    assert_eq!(listing(&b)["files"], json!(["only-b.txt"]));
}

/// direct 与 recursive 两种模式必须有确定的分组、排序和层级语义。
#[tokio::test]
async fn source_list_dir_honors_recursive_and_deterministic_ordering() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("tree")).unwrap();
    fs::create_dir(root.join("tree/z-dir")).unwrap();
    fs::create_dir(root.join("tree/a-dir")).unwrap();
    fs::write(root.join("tree/z-file.txt"), "z").unwrap();
    fs::write(root.join("tree/a-file.txt"), "a").unwrap();
    fs::write(root.join("tree/a-dir/nested.txt"), "nested").unwrap();
    let lease = lease(&root, "workspace-a", 12);

    let direct = list_at(&lease, arguments("tree")).await.unwrap();
    assert_eq!(
        listing(&direct),
        json!({"dirs":[path(&["tree", "a-dir"]),path(&["tree", "z-dir"])],"files":[path(&["tree", "a-file.txt"]),path(&["tree", "z-file.txt"]) ]})
    );

    let mut recursive_arguments = arguments("tree");
    recursive_arguments.recursive = Some(true);
    let recursive = list_at(&lease, recursive_arguments).await.unwrap();
    assert_eq!(
        listing(&recursive),
        json!({"dirs":[path(&["tree", "a-dir"]),path(&["tree", "z-dir"])],"files":[path(&["tree", "a-dir", "nested.txt"]),path(&["tree", "a-file.txt"]),path(&["tree", "z-file.txt"]) ]})
    );
}

/// hidden 与 Git ignored 条目属于 list_dir 的可见内容，不能复用 find/search 的过滤规则。
#[tokio::test]
async fn source_list_dir_keeps_hidden_and_gitignored_entries_visible() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("tree")).unwrap();
    fs::create_dir(root.join("tree/.hidden")).unwrap();
    fs::write(root.join("tree/.gitignore"), "ignored.txt\n").unwrap();
    fs::write(root.join("tree/ignored.txt"), "ignored").unwrap();
    let output = list_at(&lease(&root, "workspace-a", 12), arguments("tree"))
        .await
        .unwrap();
    let listing = listing(&output);
    assert_eq!(listing["dirs"], json!([path(&["tree", ".hidden"])]));
    assert_eq!(
        listing["files"],
        json!([
            path(&["tree", ".gitignore"]),
            path(&["tree", "ignored.txt"])
        ])
    );
}

/// 每个公开 max_bytes 值必须保留 compatibility 默认和合法范围，并始终输出完整 JSON。
#[tokio::test]
async fn source_list_dir_enforces_budget_and_never_breaks_json() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("tree")).unwrap();
    fs::write(root.join("tree/file.txt"), "file").unwrap();
    let lease = lease(&root, "workspace-a", 12);

    for max_bytes in [None, Some(65_536), Some(262_144)] {
        let mut args = arguments("tree");
        args.max_bytes = max_bytes;
        let output = list_at(&lease, args).await.unwrap();
        assert_eq!(output["truncated"], false);
        assert_eq!(
            listing(&output)["files"],
            json!([path(&["tree", "file.txt"])])
        );
    }
    let mut tiny = arguments("tree");
    tiny.max_bytes = Some(1);
    let tiny = list_at(&lease, tiny).await.unwrap();
    assert_eq!(tiny["truncated"], true);
    assert_eq!(listing(&tiny), json!({"dirs":[],"files":[]}));
    for max_bytes in [Some(0), Some(262_145)] {
        let mut args = arguments("tree");
        args.max_bytes = max_bytes;
        assert_eq!(
            list_at(&lease, args).await,
            Err("INVALID_PARAMS: max_bytes 超出范围".into())
        );
    }
}

/// 空路径、绝对、UNC、drive 与 parent 均由唯一 Resolver 拒绝，文件目标则报稳定目录错误。
#[tokio::test]
async fn source_list_dir_rejects_invalid_paths_and_non_directories() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("file.txt"), "file").unwrap();
    let lease = lease(&root, "workspace-a", 12);

    for path in ["", "..", "../outside", "C:/outside", "\\\\server\\share"] {
        assert!(
            list_at(&lease, arguments(path))
                .await
                .unwrap_err()
                .starts_with("INVALID_PATH"),
            "{path}"
        );
    }
    assert_eq!(
        list_at(&lease, arguments("file.txt")).await,
        Err("INVALID_PATH: expected a directory".into())
    );
}

/// 取消在 blocking traversal 开始前必须立即返回。
#[tokio::test]
async fn source_list_dir_stops_when_cancelled_before_work() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("tree")).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();

    assert_eq!(
        list(&lease(&root, "workspace-a", 12), arguments("tree"), cancel).await,
        Err("CANCELLED".into())
    );
}

/// 真实目录循环在同一个 Token 取消后停止，而非让 detached traversal 继续无界扫描。
#[test]
fn source_list_dir_stops_when_cancelled_during_traversal() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("tree")).unwrap();
    fs::write(root.join("tree/file.txt"), "file").unwrap();
    let lease = lease(&root, "workspace-a", 12);
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let (entered_sender, entered) = mpsc::channel();
    let (release_sender, release) = mpsc::channel();

    let worker = std::thread::spawn(move || {
        list_with_limits_and_hook(
            &lease,
            "tree",
            false,
            65_536,
            &worker_cancel,
            TraversalLimits::production(),
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
        Ok(_) => panic!("cancelled traversal must not produce a result"),
    }
}

/// entries、depth 与 deadline 三种内部硬界都必须以完整 JSON 加 truncated 收敛。
#[test]
fn source_list_dir_honors_internal_traversal_bounds() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("tree")).unwrap();
    fs::create_dir(root.join("tree/child")).unwrap();
    fs::write(root.join("tree/one.txt"), "one").unwrap();
    fs::write(root.join("tree/child/nested.txt"), "nested").unwrap();
    let lease = lease(&root, "workspace-a", 12);
    let cancel = CancellationToken::new();

    let entry_bound = list_with_limits(
        &lease,
        "tree",
        true,
        65_536,
        &cancel,
        TraversalLimits {
            max_entries: 1,
            max_depth: 64,
            timeout: Duration::from_secs(1),
        },
    )
    .unwrap();
    assert!(entry_bound.truncated);
    serde_json::from_str::<Value>(&entry_bound.text).unwrap();

    let depth_bound = list_with_limits(
        &lease,
        "tree",
        true,
        65_536,
        &cancel,
        TraversalLimits {
            max_entries: 100,
            max_depth: 0,
            timeout: Duration::from_secs(1),
        },
    )
    .unwrap();
    assert!(depth_bound.truncated);
    assert!(!depth_bound.text.contains("nested.txt"));

    let deadline_bound = list_with_limits(
        &lease,
        "tree",
        true,
        65_536,
        &cancel,
        TraversalLimits {
            max_entries: 100,
            max_depth: 64,
            timeout: Duration::ZERO,
        },
    )
    .unwrap();
    assert!(deadline_bound.truncated);
    serde_json::from_str::<Value>(&deadline_bound.text).unwrap();
}

/// A/B captured Lease 的根目录与 provenance 必须独立，不能读取全局 selection。
#[tokio::test]
async fn source_list_dir_keeps_captured_lease_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let root_a = directory.path().join("a");
    let root_b = directory.path().join("b");
    for (root, marker) in [(&root_a, "a"), (&root_b, "b")] {
        fs::create_dir(root).unwrap();
        fs::create_dir(root.join("tree")).unwrap();
        fs::write(root.join("tree/marker.txt"), marker).unwrap();
    }
    let lease_a = lease(&root_a, "workspace-a", 12);
    let lease_b = lease(&root_b, "workspace-b", 31);
    let (a, b) = tokio::join!(
        list_at(&lease_a, arguments("tree")),
        list_at(&lease_b, arguments("tree")),
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a["workspace"], json!({"id":"workspace-a","generation":12}));
    assert_eq!(b["workspace"], json!({"id":"workspace-b","generation":31}));
    assert_eq!(listing(&a)["files"], json!([path(&["tree", "marker.txt"])]));
    assert_eq!(listing(&b)["files"], json!([path(&["tree", "marker.txt"])]));
}

/// Unix symlink 可以列出但不得递归到 Workspace 外部。
#[cfg(unix)]
#[tokio::test]
async fn source_list_dir_lists_but_never_follows_outside_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    let outside = directory.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::create_dir(root.join("tree")).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    symlink(&outside, root.join("tree/link")).unwrap();
    let mut args = arguments("tree");
    args.recursive = Some(true);
    let output = list_at(&lease(&root, "workspace-a", 12), args)
        .await
        .unwrap();
    let text = output["text"].as_str().unwrap();
    assert!(text.contains("tree/link"));
    assert!(!text.contains("secret.txt"));
}

/// Windows junction/reparse point 可以列出但不得递归或泄露 root 外 children。
#[cfg(windows)]
#[tokio::test]
async fn source_list_dir_lists_but_never_follows_outside_junction() {
    use std::os::windows::process::CommandExt;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    let outside = directory.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::create_dir(root.join("tree")).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    let link = root.join("tree/link");
    let command = format!(
        "/c mklink /J \"{}\" \"{}\"",
        link.display(),
        outside.display()
    );
    let status = std::process::Command::new("cmd.exe")
        .raw_arg(command)
        .status()
        .unwrap();
    assert!(status.success());
    let mut args = arguments("tree");
    args.recursive = Some(true);
    let output = list_at(&lease(&root, "workspace-a", 12), args)
        .await
        .unwrap();
    let listing = listing(&output);
    let link = json!(path(&["tree", "link"]));
    assert!(
        listing["dirs"]
            .as_array()
            .unwrap()
            .iter()
            .chain(listing["files"].as_array().unwrap())
            .any(|entry| entry == &link)
    );
    assert!(!output["text"].as_str().unwrap().contains("secret.txt"));
}
