use super::*;
use serde_json::json;
use std::{
    fs,
    path::{MAIN_SEPARATOR, Path},
    sync::mpsc,
    time::Duration,
};

/// 创建只含 captured Lease 的 fixture，证明搜索不依赖 Desktop、Serena 或全局状态。
fn lease(root: &Path, id: &str, generation: u64) -> WorkspaceLease {
    WorkspaceLease {
        workspace_id: id.into(),
        canonical_root: root.canonicalize().unwrap(),
        generation,
    }
}

/// 构造冻结 public 字段的最小搜索参数；relative_path 缺省代表 Lease root。
fn arguments(pattern: &str) -> SourceArgs {
    SourceArgs {
        substring_pattern: Some(pattern.into()),
        ..Default::default()
    }
}

/// 通过公开 handler 调用，覆盖 DTO、budget 与 cancellation 的真实入口。
async fn search_at(lease: &WorkspaceLease, arguments: SourceArgs) -> Result<Value, String> {
    search(lease, arguments, CancellationToken::new()).await
}

/// 当前平台分隔符必须保持 Source compatibility 的相对路径文本形态。
fn path(parts: &[&str]) -> String {
    parts.join(&MAIN_SEPARATOR.to_string())
}

/// 搜索 text 是没有包装字段的稳定 JSON map，不能泄露 absolute root。
fn matches(output: &Value) -> Value {
    let matches: Value = serde_json::from_str(output["text"].as_str().unwrap()).unwrap();
    assert!(matches.is_object());
    matches
}

/// literal、regex、anchor、大小写、CRLF 与 0-based padded 行号都保持 Host 实测形态。
#[tokio::test]
async fn source_search_pattern_preserves_line_regex_and_text_shape() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(
        root.join("src/one.rs"),
        "use crate::one;\r\nWorkspacePathResolver\r\nmatch alpha\n",
    )
    .unwrap();
    fs::write(root.join("src/two.rs"), "use crate::two;\nmatch beta\n").unwrap();
    let lease = lease(&root, "workspace-a", 12);

    let anchored = search_at(&lease, arguments("^use crate")).await.unwrap();
    assert_eq!(
        matches(&anchored),
        json!({
            path(&["src", "one.rs"]): ["  >   0:use crate::one;"],
            path(&["src", "two.rs"]): ["  >   0:use crate::two;"],
        })
    );
    assert_eq!(anchored["truncated"], false);
    let grouped = search_at(&lease, arguments("^match (alpha|beta)$"))
        .await
        .unwrap();
    assert_eq!(
        matches(&grouped),
        json!({
            path(&["src", "one.rs"]): ["  >   2:match alpha"],
            path(&["src", "two.rs"]): ["  >   1:match beta"],
        })
    );
    assert_eq!(
        matches(
            &search_at(&lease, arguments("workspacepathresolver"))
                .await
                .unwrap()
        ),
        json!({})
    );
    assert_eq!(
        matches(&search_at(&lease, arguments("not-present")).await.unwrap()),
        json!({})
    );
    assert_eq!(
        matches(&search_at(&lease, arguments("[")).await.unwrap()),
        json!({})
    );
}

/// 缺省 root、显式 nested 与显式 hidden subtree 分别遵守冻结的搜索边界。
#[tokio::test]
async fn source_search_pattern_scopes_root_nested_and_explicit_hidden_subtree() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("nested")).unwrap();
    fs::create_dir(root.join(".serena")).unwrap();
    fs::write(root.join("root.txt"), "needle root\n").unwrap();
    fs::write(root.join("nested/child.txt"), "needle child\n").unwrap();
    fs::write(root.join(".serena/secret.txt"), "needle hidden\n").unwrap();
    let lease = lease(&root, "workspace-a", 12);

    let root_result = search_at(&lease, arguments("needle")).await.unwrap();
    assert_eq!(
        matches(&root_result),
        json!({
            path(&["nested", "child.txt"]): ["  >   0:needle child"],
            "root.txt": ["  >   0:needle root"],
        })
    );
    let mut nested = arguments("needle");
    nested.relative_path = Some("nested".into());
    assert_eq!(
        matches(&search_at(&lease, nested).await.unwrap()),
        json!({path(&["nested", "child.txt"]): ["  >   0:needle child"]})
    );
    let mut hidden = arguments("needle");
    hidden.relative_path = Some(".serena".into());
    assert_eq!(
        matches(&search_at(&lease, hidden).await.unwrap()),
        json!({path(&[".serena", "secret.txt"]): ["  >   0:needle hidden"]})
    );
}

/// 只读取 Workspace-local root/nested ignore；negation 生效而祖先规则不能介入。
#[tokio::test]
async fn source_search_pattern_respects_workspace_gitignore_and_ordering() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(directory.path().join(".gitignore"), "*.txt\n").unwrap();
    fs::write(
        root.join(".gitignore"),
        "ignored.txt\n*.tmp\n!important.tmp\n",
    )
    .unwrap();
    fs::write(root.join("ignored.txt"), "needle ignored\n").unwrap();
    fs::write(root.join("outside-rule.txt"), "needle visible\n").unwrap();
    fs::write(root.join("discard.tmp"), "needle ignored\n").unwrap();
    fs::write(root.join("important.tmp"), "needle important\n").unwrap();
    fs::write(root.join("nested/.gitignore"), "nested-ignored.txt\n").unwrap();
    fs::write(root.join("nested/nested-ignored.txt"), "needle ignored\n").unwrap();
    fs::write(root.join("nested/a.txt"), "needle a\n").unwrap();
    let output = search_at(&lease(&root, "workspace-a", 12), arguments("needle"))
        .await
        .unwrap();
    assert_eq!(
        matches(&output),
        json!({
            "important.tmp": ["  >   0:needle important"],
            path(&["nested", "a.txt"]): ["  >   0:needle a"],
            "outside-rule.txt": ["  >   0:needle visible"],
        })
    );
}

/// NUL 或非法 UTF-8 即使出现在早期命中之后，也必须跳过整个文件正文。
#[tokio::test]
async fn source_search_pattern_skips_binary_and_invalid_utf8_whole_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("good.png"), "needle ordinary filename\n").unwrap();
    fs::write(root.join("late-nul.txt"), b"needle before\n\0after").unwrap();
    fs::write(root.join("late-invalid.txt"), b"needle before\n\xffafter").unwrap();
    let output = search_at(&lease(&root, "workspace-a", 12), arguments("needle"))
        .await
        .unwrap();
    assert_eq!(
        matches(&output),
        json!({"good.png": ["  >   0:needle ordinary filename"]})
    );
    assert_eq!(output["truncated"], false);
}

/// path、目录目标和公开 budget 都必须严格保持既有参数边界。
#[tokio::test]
async fn source_search_pattern_rejects_invalid_paths_and_enforces_budgets() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("file.txt"), "needle\n").unwrap();
    let lease = lease(&root, "workspace-a", 12);
    for relative_path in [
        "",
        ".",
        "..",
        "../outside",
        "C:/outside",
        "\\\\server\\share",
    ] {
        let mut args = arguments("needle");
        args.relative_path = Some(relative_path.into());
        assert!(
            search_at(&lease, args)
                .await
                .unwrap_err()
                .starts_with("INVALID_PATH")
        );
    }
    let mut invalid_regex_with_bad_path = arguments("[");
    invalid_regex_with_bad_path.relative_path = Some("..".into());
    assert!(
        search_at(&lease, invalid_regex_with_bad_path)
            .await
            .unwrap_err()
            .starts_with("INVALID_PATH")
    );
    let mut file_target = arguments("needle");
    file_target.relative_path = Some("file.txt".into());
    assert_eq!(
        search_at(&lease, file_target).await,
        Err("INVALID_PATH: expected a directory".into())
    );
    for max_bytes in [None, Some(65_536), Some(262_144)] {
        let mut args = arguments("needle");
        args.max_bytes = max_bytes;
        assert_eq!(search_at(&lease, args).await.unwrap()["truncated"], false);
    }
    let mut tiny = arguments("needle");
    tiny.max_bytes = Some(1);
    let tiny = search_at(&lease, tiny).await.unwrap();
    assert_eq!(matches(&tiny), json!({}));
    assert_eq!(tiny["truncated"], true);
    for max_bytes in [Some(0), Some(262_145)] {
        let mut args = arguments("needle");
        args.max_bytes = max_bytes;
        assert_eq!(
            search_at(&lease, args).await,
            Err("INVALID_PARAMS: max_bytes 超出范围".into())
        );
    }
}

/// file/entry/depth/match/deadline 的硬界均以合法 JSON 与 truncated 收敛。
#[test]
fn source_search_pattern_honors_internal_bounds() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("tree")).unwrap();
    fs::create_dir(root.join("tree/child")).unwrap();
    fs::write(root.join("tree/one.txt"), "needle\n").unwrap();
    fs::write(root.join("tree/child/two.txt"), "needle\n").unwrap();
    fs::write(root.join("large.txt"), "needle\nmore").unwrap();
    let lease = lease(&root, "workspace-a", 12);
    let cancel = CancellationToken::new();
    let limits = |max_file_bytes, max_entries, max_matches, max_depth, timeout| SearchLimits {
        max_file_bytes,
        max_entries,
        max_matches,
        max_depth,
        timeout,
    };
    for result in [
        search_with_limits(
            &lease,
            None,
            "needle",
            65_536,
            &cancel,
            limits(3, 100, 100, 64, Duration::from_secs(1)),
        )
        .unwrap(),
        search_with_limits(
            &lease,
            Some("tree"),
            "needle",
            65_536,
            &cancel,
            limits(1024, 1, 100, 64, Duration::from_secs(1)),
        )
        .unwrap(),
        search_with_limits(
            &lease,
            Some("tree"),
            "needle",
            65_536,
            &cancel,
            limits(1024, 100, 100, 0, Duration::from_secs(1)),
        )
        .unwrap(),
        search_with_limits(
            &lease,
            Some("tree"),
            "needle",
            65_536,
            &cancel,
            limits(1024, 100, 1, 64, Duration::from_secs(1)),
        )
        .unwrap(),
        search_with_limits(
            &lease,
            Some("tree"),
            "needle",
            65_536,
            &cancel,
            limits(1024, 100, 100, 64, Duration::ZERO),
        )
        .unwrap(),
    ] {
        assert!(result.truncated);
        serde_json::from_str::<Value>(&result.text).unwrap();
    }
}

/// work 前、traversal 与大文件无命中读取期间均使用同一 token 及时取消。
#[tokio::test]
async fn source_search_pattern_stops_on_cancellation_before_and_during_work() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("large.txt"), "x".repeat(64 * 1024)).unwrap();
    let captured = lease(&root, "workspace-a", 12);
    let before = CancellationToken::new();
    before.cancel();
    assert_eq!(
        search(&captured, arguments("needle"), before).await,
        Err("CANCELLED".into())
    );

    let traversal_cancel = CancellationToken::new();
    let worker_cancel = traversal_cancel.clone();
    let traversal_lease = captured.clone();
    let (entered_sender, entered) = mpsc::channel();
    let (release_sender, release) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        search_with_limits_and_hooks(
            &traversal_lease,
            None,
            "needle",
            65_536,
            &worker_cancel,
            SearchLimits::production(),
            move |_| {
                entered_sender.send(()).unwrap();
                release.recv().unwrap();
            },
            || {},
        )
    });
    entered.recv_timeout(Duration::from_secs(1)).unwrap();
    traversal_cancel.cancel();
    release_sender.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Err(error) if error == "CANCELLED"));

    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let (entered_sender, entered) = mpsc::channel();
    let (release_sender, release) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        search_with_limits_and_hooks(
            &captured,
            None,
            "needle",
            65_536,
            &worker_cancel,
            SearchLimits::production(),
            |_| {},
            move || {
                entered_sender.send(()).unwrap();
                release.recv().unwrap();
            },
        )
    });
    entered.recv_timeout(Duration::from_secs(1)).unwrap();
    cancel.cancel();
    release_sender.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Err(error) if error == "CANCELLED"));
}

/// A/B 并发搜索必须只从各自 captured Lease root 返回 provenance 与内容。
#[tokio::test]
async fn source_search_pattern_keeps_captured_lease_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let root_a = directory.path().join("a");
    let root_b = directory.path().join("b");
    fs::create_dir(&root_a).unwrap();
    fs::create_dir(&root_b).unwrap();
    fs::write(root_a.join("marker.txt"), "needle A\n").unwrap();
    fs::write(root_b.join("marker.txt"), "needle B\n").unwrap();
    let lease_a = lease(&root_a, "workspace-a", 12);
    let lease_b = lease(&root_b, "workspace-b", 31);
    let (a, b) = tokio::join!(
        search_at(&lease_a, arguments("needle A")),
        search_at(&lease_b, arguments("needle B")),
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a["workspace"], json!({"id":"workspace-a","generation":12}));
    assert_eq!(b["workspace"], json!({"id":"workspace-b","generation":31}));
    assert_eq!(matches(&a), json!({"marker.txt": ["  >   0:needle A"]}));
    assert_eq!(matches(&b), json!({"marker.txt": ["  >   0:needle B"]}));
}

/// Unix symlink 不能成为搜索根、结果或通往 Workspace 外 secret 的递归入口。
#[cfg(unix)]
#[tokio::test]
async fn source_search_pattern_never_follows_outside_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    let outside = directory.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "needle secret\n").unwrap();
    symlink(&outside, root.join("link")).unwrap();
    let output = search_at(&lease(&root, "workspace-a", 12), arguments("needle"))
        .await
        .unwrap();
    assert_eq!(matches(&output), json!({}));
}

/// Windows junction/reparse point 不能递归并泄露 Workspace 外内容。
#[cfg(windows)]
#[tokio::test]
async fn source_search_pattern_never_follows_outside_junction() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    let outside = directory.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "needle secret\n").unwrap();
    let link = root.join("link");
    let status = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(&outside)
        .status()
        .unwrap();
    assert!(status.success());
    let output = search_at(&lease(&root, "workspace-a", 12), arguments("needle"))
        .await
        .unwrap();
    assert_eq!(matches(&output), json!({}));
}
