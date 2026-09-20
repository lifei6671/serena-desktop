use super::*;
use serde_json::json;
use std::{
    fs,
    path::{MAIN_SEPARATOR_STR, Path},
    sync::mpsc,
    time::Duration,
};

/// 创建只含 captured Lease 的 fixture，证明查找不依赖 Desktop、Serena 或 global state。
fn lease(root: &Path, id: &str, generation: u64) -> WorkspaceLease {
    WorkspaceLease {
        workspace_id: id.into(),
        canonical_root: root.canonicalize().unwrap(),
        generation,
    }
}

/// 构造冻结 public 字段名的最小查找参数；relative_path 缺省即代表 Lease root。
fn arguments(file_mask: &str) -> SourceArgs {
    SourceArgs {
        file_mask: Some(file_mask.into()),
        ..Default::default()
    }
}

/// 调用公开 handler，确保 DTO、budget 与 cancellation 都走真实入口。
async fn find_at(lease: &WorkspaceLease, arguments: SourceArgs) -> Result<Value, String> {
    find(lease, arguments, CancellationToken::new()).await
}

/// 将稳定 JSON text 的 files 字段读成数组，并拒绝额外公开字段。
fn files(output: &Value) -> Value {
    let files: Value = serde_json::from_str(output["text"].as_str().unwrap()).unwrap();
    assert_eq!(files.as_object().unwrap().len(), 1);
    assert!(files["files"].is_array());
    files["files"].clone()
}

/// 当前平台的相对路径分隔符必须与既有 Source 文本兼容。
fn path(parts: &[&str]) -> String {
    parts.join(MAIN_SEPARATOR_STR)
}

/// exact、wildcard、no-match 与 basename-only 均不读取文件正文。
#[tokio::test]
async fn source_find_file_matches_exact_wildcard_and_basename_only() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/exact.rs"), b"\0binary body").unwrap();
    fs::write(root.join("src/wild.rs"), "text").unwrap();
    fs::write(root.join("src/not-rs.txt"), "text").unwrap();
    let lease = lease(&root, "workspace-a", 12);

    let exact = find_at(&lease, arguments("exact.rs")).await.unwrap();
    assert_eq!(files(&exact), json!([path(&["src", "exact.rs"])]));

    let wildcard = find_at(&lease, arguments("*.rs")).await.unwrap();
    assert_eq!(
        files(&wildcard),
        json!([path(&["src", "exact.rs"]), path(&["src", "wild.rs"])])
    );

    let basename = find_at(&lease, arguments("src/*.rs")).await.unwrap();
    assert_eq!(files(&basename), json!([]));
    assert_eq!(basename["truncated"], false);
}

/// 缺省 relative_path 从 Lease root 搜索，显式 nested path 仅搜索该子树。
#[tokio::test]
async fn source_find_file_uses_root_when_relative_path_is_omitted_and_limits_nested_subtree() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(root.join("root.rs"), "root").unwrap();
    fs::write(root.join("nested/child.rs"), "child").unwrap();
    let lease = lease(&root, "workspace-a", 12);

    let root_result = find_at(&lease, arguments("*.rs")).await.unwrap();
    assert_eq!(
        files(&root_result),
        json!([path(&["nested", "child.rs"]), "root.rs"])
    );
    let mut nested = arguments("*.rs");
    nested.relative_path = Some("nested".into());
    let nested_result = find_at(&lease, nested).await.unwrap();
    assert_eq!(
        files(&nested_result),
        json!([path(&["nested", "child.rs"])])
    );
}

/// Workspace-local root/nested ignore 与 negation 必须生效，Workspace 外父规则不得参与。
#[tokio::test]
async fn source_find_file_respects_only_workspace_gitignore_rules() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(directory.path().join(".gitignore"), "*.rs\n").unwrap();
    fs::write(
        root.join(".gitignore"),
        "root-ignored.rs\n*.tmp\n!important.tmp\n",
    )
    .unwrap();
    fs::write(root.join("root-ignored.rs"), "ignored").unwrap();
    fs::write(root.join("outside-rule.rs"), "must remain visible").unwrap();
    fs::write(root.join("discard.tmp"), "ignored").unwrap();
    fs::write(root.join("important.tmp"), "included").unwrap();
    fs::write(root.join("nested/.gitignore"), "nested-ignored.rs\n").unwrap();
    fs::write(root.join("nested/nested-ignored.rs"), "ignored").unwrap();
    fs::write(root.join("nested/visible.rs"), "visible").unwrap();
    let lease = lease(&root, "workspace-a", 12);

    let rs = find_at(&lease, arguments("*.rs")).await.unwrap();
    assert_eq!(
        files(&rs),
        json!([path(&["nested", "visible.rs"]), "outside-rule.rs"])
    );
    let tmp = find_at(&lease, arguments("*.tmp")).await.unwrap();
    assert_eq!(files(&tmp), json!(["important.tmp"]));
}

/// dotfile 与 hidden directory 默认完全跳过，不能通过文件名 glob 绕过。
#[tokio::test]
async fn source_find_file_skips_hidden_files_and_directories() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join(".hidden")).unwrap();
    fs::write(root.join(".dot.rs"), "hidden").unwrap();
    fs::write(root.join(".hidden/secret.rs"), "hidden").unwrap();
    fs::write(root.join("visible.rs"), "visible").unwrap();

    let output = find_at(&lease(&root, "workspace-a", 12), arguments("*.rs"))
        .await
        .unwrap();
    assert_eq!(files(&output), json!(["visible.rs"]));
}

/// Windows 兼容性明确要求 file_mask case-insensitive，其他平台保留原有大小写语义。
#[cfg(windows)]
#[tokio::test]
async fn source_find_file_matches_masks_case_insensitively_on_windows() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("source_File.RS"), "text").unwrap();
    let lease = lease(&root, "workspace-a", 12);

    let lower = find_at(&lease, arguments("source_*.rs")).await.unwrap();
    let upper = find_at(&lease, arguments("SOURCE_*.RS")).await.unwrap();
    assert_eq!(files(&lower), files(&upper));
}

/// 非 Windows 不强行改写已有平台大小写语义。
#[cfg(not(windows))]
#[tokio::test]
async fn source_find_file_preserves_case_sensitive_masks_off_windows() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("source_File.RS"), "text").unwrap();
    let output = find_at(&lease(&root, "workspace-a", 12), arguments("source_*.rs"))
        .await
        .unwrap();
    assert_eq!(files(&output), json!([]));
}

/// 非法路径、文件 target 与非法 glob 必须稳定 fail closed。
#[tokio::test]
async fn source_find_file_rejects_invalid_paths_non_directories_and_globs() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("file.rs"), "file").unwrap();
    let lease = lease(&root, "workspace-a", 12);

    for relative_path in [
        "",
        ".",
        "..",
        "../outside",
        "/outside",
        "C:/outside",
        "\\\\server\\share",
    ] {
        let mut args = arguments("*.rs");
        args.relative_path = Some(relative_path.into());
        assert!(
            find_at(&lease, args)
                .await
                .unwrap_err()
                .starts_with("INVALID_PATH"),
            "{relative_path}"
        );
    }
    let mut file_target = arguments("*.rs");
    file_target.relative_path = Some("file.rs".into());
    assert_eq!(
        find_at(&lease, file_target).await,
        Err("INVALID_PATH: expected a directory".into())
    );
    assert_eq!(
        find_at(&lease, arguments("[")).await,
        Err("INVALID_PARAMS: invalid file_mask".into())
    );
}

/// budget 的缺省、边界、错误与 tiny JSON 收敛必须符合 compatibility contract。
#[tokio::test]
async fn source_find_file_enforces_budgets_and_never_breaks_json() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("file.rs"), "file").unwrap();
    let lease = lease(&root, "workspace-a", 12);

    for max_bytes in [None, Some(65_536), Some(262_144)] {
        let mut args = arguments("*.rs");
        args.max_bytes = max_bytes;
        let output = find_at(&lease, args).await.unwrap();
        assert_eq!(output["truncated"], false);
        assert_eq!(files(&output), json!(["file.rs"]));
    }
    let mut tiny = arguments("*.rs");
    tiny.max_bytes = Some(1);
    let tiny = find_at(&lease, tiny).await.unwrap();
    assert_eq!(tiny["truncated"], true);
    assert_eq!(files(&tiny), json!([]));
    for max_bytes in [Some(0), Some(262_145)] {
        let mut args = arguments("*.rs");
        args.max_bytes = max_bytes;
        assert_eq!(
            find_at(&lease, args).await,
            Err("INVALID_PARAMS: max_bytes 超出范围".into())
        );
    }
}

/// entries、matches、depth 与 deadline 四种内部硬界都必须以 JSON 与 truncated 收敛。
#[test]
fn source_find_file_honors_internal_traversal_bounds() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("tree")).unwrap();
    fs::create_dir(root.join("tree/child")).unwrap();
    fs::write(root.join("tree/one.rs"), "one").unwrap();
    fs::write(root.join("tree/child/two.rs"), "two").unwrap();
    let lease = lease(&root, "workspace-a", 12);
    let cancel = CancellationToken::new();
    let limits = |max_entries, max_matches, max_depth, timeout| TraversalLimits {
        max_entries,
        max_matches,
        max_depth,
        timeout,
    };

    for limited in [
        find_with_limits(
            &lease,
            Some("tree"),
            "*.rs",
            65_536,
            &cancel,
            limits(1, 100, 64, Duration::from_secs(1)),
        )
        .unwrap(),
        find_with_limits(
            &lease,
            Some("tree"),
            "*.rs",
            65_536,
            &cancel,
            limits(100, 1, 64, Duration::from_secs(1)),
        )
        .unwrap(),
        find_with_limits(
            &lease,
            Some("tree"),
            "*.rs",
            65_536,
            &cancel,
            limits(100, 100, 0, Duration::from_secs(1)),
        )
        .unwrap(),
        find_with_limits(
            &lease,
            Some("tree"),
            "*.rs",
            65_536,
            &cancel,
            limits(100, 100, 64, Duration::ZERO),
        )
        .unwrap(),
    ] {
        assert!(limited.truncated);
        serde_json::from_str::<Value>(&limited.text).unwrap();
    }
}

/// cancellation 在 blocking work 之前必须直接返回，不能产生部分结果。
#[tokio::test]
async fn source_find_file_stops_when_cancelled_before_work() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        find(&lease(&root, "workspace-a", 12), arguments("*.rs"), cancel).await,
        Err("CANCELLED".into())
    );
}

/// traversal 中取消必须观察同一 Token，不能在 caller 返回后继续扫描。
#[test]
fn source_find_file_stops_when_cancelled_during_traversal() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("file.rs"), "file").unwrap();
    let lease = lease(&root, "workspace-a", 12);
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let (entered_sender, entered) = mpsc::channel();
    let (release_sender, release) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        find_with_limits_and_hook(
            &lease,
            None,
            "*.rs",
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

/// A/B captured Lease 的根与 provenance 必须独立，不能读取 Desktop selection。
#[tokio::test]
async fn source_find_file_keeps_captured_lease_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let root_a = directory.path().join("a");
    let root_b = directory.path().join("b");
    for (root, marker) in [(&root_a, "a"), (&root_b, "b")] {
        fs::create_dir(root).unwrap();
        fs::write(root.join("marker.rs"), marker).unwrap();
    }
    let lease_a = lease(&root_a, "workspace-a", 12);
    let lease_b = lease(&root_b, "workspace-b", 31);
    let (a, b) = tokio::join!(
        find_at(&lease_a, arguments("*.rs")),
        find_at(&lease_b, arguments("*.rs")),
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a["workspace"], json!({"id":"workspace-a","generation":12}));
    assert_eq!(b["workspace"], json!({"id":"workspace-b","generation":31}));
    assert_eq!(files(&a), json!(["marker.rs"]));
    assert_eq!(files(&b), json!(["marker.rs"]));
}

/// Unix symlink 不得作为结果或递归进入 Workspace 外的 secret child。
#[cfg(unix)]
#[tokio::test]
async fn source_find_file_never_follows_or_returns_outside_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    let outside = directory.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.rs"), "secret").unwrap();
    symlink(&outside, root.join("link.rs")).unwrap();
    let output = find_at(&lease(&root, "workspace-a", 12), arguments("*.rs"))
        .await
        .unwrap();
    assert_eq!(files(&output), json!([]));
    assert!(!output["text"].as_str().unwrap().contains("secret.rs"));
}

/// Windows junction/reparse point 不得递归或泄露 Workspace 外 children。
#[cfg(windows)]
#[tokio::test]
async fn source_find_file_never_follows_outside_junction() {
    use std::os::windows::process::CommandExt;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    let outside = directory.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.rs"), "secret").unwrap();
    let link = root.join("link");
    let command = format!(
        "/c mklink /J \"{}\" \"{}\"",
        link.display(),
        outside.display()
    );
    assert!(
        std::process::Command::new("cmd.exe")
            .raw_arg(command)
            .status()
            .unwrap()
            .success()
    );
    let output = find_at(&lease(&root, "workspace-a", 12), arguments("*.rs"))
        .await
        .unwrap();
    assert_eq!(files(&output), json!([]));
    assert!(!output["text"].as_str().unwrap().contains("secret.rs"));
}
