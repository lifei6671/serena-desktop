//! P2C-010 仅实现本地 `source_replace_lines`；Remote registry 与 Broker dispatch 保持关闭。
#![allow(
    dead_code,
    reason = "P2C-010 implements the local handler before a later task explicitly advertises Remote Source Write."
)]

use super::{
    registry,
    source_write_atomic_replace::replace_existing_file,
    source_write_commit::{TargetCommitError, lock_existing_target},
    source_write_delete::delete_closed_line_range,
    source_write_domain::{
        SourceLineRange, SourceWriteError, SourceWriteSuccess, VersionedSourceWriteTarget,
        validate_inline_mutation_content, validate_result_text_file_size,
    },
    source_write_insert::insert_normalized_lines,
    source_write_support::{
        ExistingTextSnapshotError, candidate_sha256, read_existing_text_snapshot,
        workspace_relative_path,
    },
    source_write_text::{detect_newline_style, normalize_newlines},
};
use crate::{serena::SupervisorState, workspace_path::WorkspacePathResolver};
use serde::Deserialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// `source_replace_lines` 的严格本地输入；relative_path 保持冻结 snake_case wire 名。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceReplaceLinesInput {
    #[serde(flatten)]
    target: VersionedSourceWriteTarget,
    start_line: u32,
    end_line: u32,
    content: String,
}

/// 在显式 Workspace Lease 中按 1-based inclusive closed range 替换既有 UTF-8 文本行。
pub(crate) async fn replace_lines(
    supervisor: &SupervisorState,
    arguments: Value,
    cancel: CancellationToken,
) -> Result<SourceWriteSuccess, String> {
    // Workspace taxonomy 先由既有 adapter 统一判定，serde 不得改变 missing/null 的稳定分类。
    let workspace_id = registry::parse_workspace_id(&arguments)?;
    let input: SourceReplaceLinesInput =
        serde_json::from_value(arguments).map_err(|error| format!("INVALID_PARAMS: {error}"))?;
    debug_assert_eq!(input.target.workspace_id, workspace_id);
    replace_lines_input(supervisor, input, cancel).await
}

/// schema 解析后的 handler；所有纯输入校验在 Guard 与 filesystem access 前完成。
async fn replace_lines_input(
    supervisor: &SupervisorState,
    input: SourceReplaceLinesInput,
    cancel: CancellationToken,
) -> Result<SourceWriteSuccess, String> {
    input.target.validate().map_err(source_error)?;
    validate_inline_mutation_content(&input.content).map_err(source_error)?;
    let changed_range = SourceLineRange {
        start_line: input.start_line,
        end_line: input.end_line,
    };
    // 0 与 startLine > endLine 不依赖目标行数，必须在取得任何 Workspace 或文件 authority 前稳定拒绝。
    changed_range.validate().map_err(source_error)?;
    if cancel.is_cancelled() {
        return Err("CANCELLED".into());
    }

    // 只从 caller 的显式 workspaceId 取得 captured Lease 与 RAII Guard，绝不回退到 UI 或 session state。
    let (lease, guard) = supervisor.resolve_workspace_write_guard(&input.target.workspace_id)?;
    WorkspacePathResolver::new(&lease)
        .resolve(&input.target.relative_path)
        .map_err(|_| source_error(SourceWriteError::PathOutsideWorkspace))?;

    // pre-read 只产生与 expected version 匹配的 bounded snapshot；range 边界与目标 newline policy 均只从此 snapshot 取得。
    let snapshot = read_existing_text_snapshot(
        lease.clone(),
        input.target.relative_path.clone(),
        input.target.expected_sha256.clone(),
        cancel.clone(),
    )
    .await
    .map_err(snapshot_error)?;
    let deleted = delete_closed_line_range(&snapshot.text, changed_range)?;
    let candidate = if input.content.is_empty() {
        // 空 replacement 的公开语义固定等价于同一 range 的 source_delete_lines。
        deleted
    } else {
        let newline_style = detect_newline_style(&snapshot.text);
        let normalized = normalize_newlines(&input.content, newline_style);
        // 复用 insert 的 separator ownership：range 的 trailing separator 已由 delete 消耗，bridge 仅在两侧均有文本时插入。
        insert_normalized_lines(&deleted, &normalized, input.start_line, newline_style)?.0
    };
    validate_result_text_file_size(candidate.len()).map_err(source_error)?;
    let after_sha256 = candidate_sha256(candidate.as_bytes());

    if cancel.is_cancelled() {
        return Err("CANCELLED".into());
    }
    // 同一 Lease 与 Guard 进入 P2C-003；锁内再次验证 expected SHA，闭合 pre-read 到 replace 的 OCC。
    let locked = tokio::select! {
        _ = cancel.cancelled() => return Err("CANCELLED".into()),
        result = lock_existing_target(
            supervisor,
            lease.clone(),
            guard,
            &input.target.relative_path,
            input.target.expected_sha256,
        ) => result.map_err(target_commit_error)?,
    };
    if cancel.is_cancelled() {
        return Err("CANCELLED".into());
    }
    let before_sha256 = locked
        .before_sha256()
        .cloned()
        .ok_or_else(|| source_error(SourceWriteError::IoError))?;
    let path = workspace_relative_path(&lease.canonical_root, locked.canonical_target())?;

    // 一旦进入 P2C-004 replace，不再观察 cancellation；UNKNOWN 必须原样向上投影。
    replace_existing_file(&locked, candidate.as_bytes()).map_err(source_error)?;
    let success = SourceWriteSuccess {
        path,
        workspace_id: lease.workspace_id,
        generation: lease.generation,
        before_sha256: Some(before_sha256),
        after_sha256,
        changed_range: Some(changed_range),
        changed_count: Some(input.end_line - input.start_line + 1),
    };
    success.validate().map_err(source_error)?;
    Ok(success)
}

/// 保持 P2C-001 Source Write taxonomy，snapshot 内不泄露 filesystem 错误文字。
fn snapshot_error(error: ExistingTextSnapshotError) -> String {
    match error {
        ExistingTextSnapshotError::Source(error) => source_error(error),
        ExistingTextSnapshotError::WorkspaceChanged => "WORKSPACE_CHANGED".into(),
        ExistingTextSnapshotError::Cancelled => "CANCELLED".into(),
    }
}

/// 保持 P2C-001 Source Write string taxonomy，不泄露内部 enum 名。
fn source_error(error: SourceWriteError) -> String {
    error.code().into()
}

/// P2C-003 已定义 WorkspaceChanged 与 Source Write 的唯一稳定投影。
fn target_commit_error(error: TargetCommitError) -> String {
    error.code().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{self, AppPaths, ManagerConfig, Workspace},
        mcp::{
            Broker,
            source_write_commit::{LockedTargetCommit, lock_existing_target},
            source_write_delete::delete_lines,
            source_write_support::{candidate_sha256, set_snapshot_ready_hook_for_test},
        },
        serena::SupervisorState,
    };
    use serde_json::json;
    #[cfg(windows)]
    use std::process::Command;
    use std::{
        fs,
        path::PathBuf,
        sync::{Arc, Mutex, MutexGuard, OnceLock, mpsc},
        time::Duration,
    };

    static TEST_SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

    /// 获取 P2C-010 handler 测试的唯一 hook ownership，释放后才允许下一条测试安装 hook。
    fn test_guard() -> MutexGuard<'static, ()> {
        TEST_SERIAL.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    /// 构造注册但未选择的 Workspace，证明 handler 只使用显式 workspaceId。
    fn fixture() -> (tempfile::TempDir, Arc<SupervisorState>, Workspace, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        fs::create_dir(&root).unwrap();
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs/app.log"),
            serena_log: directory.path().join("logs/serena.log"),
        };
        let workspace = Workspace {
            id: "workspace".into(),
            name: "Workspace".into(),
            root: root.clone(),
            generation: 7,
        };
        config::save(
            &paths.config_file,
            &ManagerConfig {
                workspace_registry_revision: 3,
                workspaces: vec![workspace.clone()],
                ..ManagerConfig::default()
            },
        )
        .unwrap();
        (
            directory,
            Arc::new(SupervisorState::new(paths).unwrap()),
            workspace,
            root,
        )
    }

    /// 从既有 raw bytes 生成 handler 应接受的冻结 expected version token。
    fn expected(bytes: &[u8]) -> String {
        candidate_sha256(bytes).as_str().into()
    }

    /// 以冻结 local DTO 调用 handler；测试不经由 Remote registry 或 Broker dispatch 进入 write 路径。
    async fn replace(
        supervisor: &SupervisorState,
        relative_path: &str,
        expected_sha256: String,
        start_line: u32,
        end_line: u32,
        content: &str,
        cancel: CancellationToken,
    ) -> Result<SourceWriteSuccess, String> {
        replace_lines(
            supervisor,
            json!({
                "workspaceId":"workspace",
                "relative_path":relative_path,
                "expectedSha256":expected_sha256,
                "startLine":start_line,
                "endLine":end_line,
                "content":content,
            }),
            cancel,
        )
        .await
    }

    /// 直接取得 existing lock，供 lock-wait OCC 与 cancellation 测试复用。
    async fn existing_lock(
        supervisor: &SupervisorState,
        workspace: &Workspace,
        relative_path: &str,
        bytes: &[u8],
    ) -> LockedTargetCommit {
        let (lease, guard) = supervisor
            .resolve_workspace_write_guard(&workspace.id)
            .unwrap();
        lock_existing_target(
            supervisor,
            lease,
            guard,
            relative_path,
            candidate_sha256(bytes),
        )
        .await
        .unwrap()
    }

    /// 首段、中段、末段、单行与多行 replacement 都保留 range provenance 与冻结 final-newline 归属。
    #[tokio::test]
    async fn replaces_closed_ranges_with_provenance() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for (path, existing, start_line, end_line, content, expected_bytes) in [
            (
                "first.txt",
                b"one\ntwo\n".as_slice(),
                1,
                1,
                "first",
                b"first\ntwo\n".as_slice(),
            ),
            (
                "middle.txt",
                b"one\ntwo\nthree\nfour\n".as_slice(),
                2,
                3,
                "two\nand three",
                b"one\ntwo\nand three\nfour\n".as_slice(),
            ),
            (
                "last.txt",
                b"one\ntwo\n".as_slice(),
                2,
                2,
                "last",
                b"one\nlast".as_slice(),
            ),
            (
                "single.txt",
                b"only".as_slice(),
                1,
                1,
                "next",
                b"next".as_slice(),
            ),
        ] {
            fs::write(root.join(path), existing).unwrap();
            let result = replace(
                supervisor.as_ref(),
                path,
                expected(existing),
                start_line,
                end_line,
                content,
                CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(fs::read(root.join(path)).unwrap(), expected_bytes);
            assert_eq!(result.path, path);
            assert_eq!(result.workspace_id, "workspace");
            assert_eq!(result.generation, 7);
            assert_eq!(result.before_sha256.unwrap().as_str(), expected(existing));
            assert_eq!(result.after_sha256.as_str(), expected(expected_bytes));
            assert_eq!(
                result.changed_range,
                Some(SourceLineRange {
                    start_line,
                    end_line
                })
            );
            assert_eq!(result.changed_count, Some(end_line - start_line + 1));
        }
    }

    /// EOF 替换严格采用 delete 加 insert 的 final-newline ownership，不隐式保留原 range 的末尾分隔符。
    #[tokio::test]
    async fn eof_final_newline_ownership_is_compositional() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for (path, existing, content, expected_bytes) in [
            (
                "original-final-replacement-plain.txt",
                b"one\ntwo\n".as_slice(),
                "last",
                b"one\nlast".as_slice(),
            ),
            (
                "original-final-replacement-terminal.txt",
                b"one\ntwo\n".as_slice(),
                "last\n",
                b"one\nlast\n".as_slice(),
            ),
            (
                "original-plain-replacement-plain.txt",
                b"one\ntwo".as_slice(),
                "last",
                b"one\nlast".as_slice(),
            ),
            (
                "original-plain-replacement-terminal.txt",
                b"one\ntwo".as_slice(),
                "last\n",
                b"one\nlast\n".as_slice(),
            ),
        ] {
            fs::write(root.join(path), existing).unwrap();
            replace(
                supervisor.as_ref(),
                path,
                expected(existing),
                2,
                2,
                content,
                CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(fs::read(root.join(path)).unwrap(), expected_bytes);
        }
    }

    /// 空 replacement 必须与同一 range 的 source_delete_lines 产生逐字节一致的结果。
    #[tokio::test]
    async fn empty_replacement_is_byte_equivalent_to_delete_lines() {
        let (_directory, supervisor, _workspace, root) = fixture();
        let existing = b"one\r\ntwo\nthree\r\nfour";
        fs::write(root.join("replace.txt"), existing).unwrap();
        fs::write(root.join("delete.txt"), existing).unwrap();
        replace(
            supervisor.as_ref(),
            "replace.txt",
            expected(existing),
            2,
            3,
            "",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        delete_lines(supervisor.as_ref(), json!({"workspaceId":"workspace", "relative_path":"delete.txt", "expectedSha256":expected(existing), "startLine":2, "endLine":3}), CancellationToken::new()).await.unwrap();
        assert_eq!(
            fs::read(root.join("replace.txt")).unwrap(),
            fs::read(root.join("delete.txt")).unwrap()
        );
    }

    /// LF、CRLF 与 mixed target 均只规范化 replacement；未替换区域 raw bytes 与 target policy 保持不变。
    #[tokio::test]
    async fn normalizes_replacement_without_rewriting_untouched_bytes() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for (path, existing, content, expected_bytes) in [
            (
                "lf.txt",
                b"one\ntwo\nthree\n".as_slice(),
                "a\r\nb",
                b"one\na\nb\nthree\n".as_slice(),
            ),
            (
                "crlf.txt",
                b"one\r\ntwo\r\nthree\r\n".as_slice(),
                "a\nb",
                b"one\r\na\r\nb\r\nthree\r\n".as_slice(),
            ),
            (
                "mixed.txt",
                b"one\r\ntwo\nthree\r\nfour".as_slice(),
                "a\nb",
                b"one\r\na\r\nb\r\nthree\r\nfour".as_slice(),
            ),
        ] {
            fs::write(root.join(path), existing).unwrap();
            replace(
                supervisor.as_ref(),
                path,
                expected(existing),
                2,
                2,
                content,
                CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(fs::read(root.join(path)).unwrap(), expected_bytes);
        }
    }

    /// 纯非法 range 优先于 Workspace、路径和文件访问；endLine > N 仅在 bounded snapshot 后拒绝。
    #[tokio::test]
    async fn rejects_invalid_ranges_before_access_and_after_snapshot_when_needed() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for (start_line, end_line) in [(0, 1), (1, 0), (2, 1)] {
            assert_eq!(
                replace(
                    supervisor.as_ref(),
                    "missing.txt",
                    expected(b"irrelevant"),
                    start_line,
                    end_line,
                    "x",
                    CancellationToken::new()
                )
                .await,
                Err("SOURCE_RANGE_INVALID".into())
            );
        }
        assert!(!root.join("missing.txt").exists());
        let existing = b"one\ntwo";
        fs::write(root.join("lines.txt"), existing).unwrap();
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "lines.txt",
                expected(existing),
                1,
                3,
                "x",
                CancellationToken::new()
            )
            .await,
            Err("SOURCE_RANGE_INVALID".into())
        );
        assert_eq!(fs::read(root.join("lines.txt")).unwrap(), existing);
    }

    /// stale SHA、NUL、inline input、target 与 result hard limit 均不得进入 replace。
    #[tokio::test]
    async fn rejects_stale_nul_and_all_size_limits_without_commit() {
        let (_directory, supervisor, _workspace, root) = fixture();
        fs::write(root.join("stale.txt"), b"current\n").unwrap();
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "stale.txt",
                expected(b"old\n"),
                1,
                1,
                "new",
                CancellationToken::new()
            )
            .await,
            Err("SOURCE_VERSION_CONFLICT".into())
        );
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "missing.txt",
                expected(b"unused"),
                1,
                1,
                "bad\0content",
                CancellationToken::new()
            )
            .await,
            Err("SOURCE_BINARY_REJECTED".into())
        );
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "missing.txt",
                expected(b"unused"),
                1,
                1,
                &"x".repeat(1024 * 1024 + 1),
                CancellationToken::new()
            )
            .await,
            Err("SOURCE_INPUT_LIMIT_EXCEEDED".into())
        );
        let oversized = vec![b'x'; 8 * 1024 * 1024 + 1];
        fs::write(root.join("oversized.txt"), &oversized).unwrap();
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "oversized.txt",
                expected(&oversized),
                1,
                1,
                "new",
                CancellationToken::new()
            )
            .await,
            Err("SOURCE_FILE_TOO_LARGE".into())
        );
        let result_source = format!("a\n{}", "x".repeat(8 * 1024 * 1024 - 2));
        fs::write(root.join("result.txt"), &result_source).unwrap();
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "result.txt",
                expected(result_source.as_bytes()),
                1,
                1,
                &"y".repeat(1024 * 1024),
                CancellationToken::new()
            )
            .await,
            Err("SOURCE_FILE_TOO_LARGE".into())
        );
        assert_eq!(
            fs::read(root.join("result.txt")).unwrap(),
            result_source.as_bytes()
        );
    }

    /// pre-read 后的 external edit 必须由 P2C-003 locked revalidation 拒绝，不能覆盖外部 bytes。
    #[tokio::test]
    async fn external_edit_after_preread_is_not_overwritten() {
        let _guard = test_guard();
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("race.txt");
        fs::write(&target, b"one\ntwo\n").unwrap();
        let holder =
            existing_lock(supervisor.as_ref(), &workspace, "race.txt", b"one\ntwo\n").await;
        let (ready_tx, ready_rx) = mpsc::channel();
        set_snapshot_ready_hook_for_test(Arc::new(move || ready_tx.send(()).unwrap()));
        let task_supervisor = Arc::clone(&supervisor);
        let task = tokio::spawn(async move {
            replace(
                task_supervisor.as_ref(),
                "race.txt",
                expected(b"one\ntwo\n"),
                1,
                1,
                "candidate",
                CancellationToken::new(),
            )
            .await
        });
        tokio::task::spawn_blocking(move || ready_rx.recv_timeout(Duration::from_secs(2)))
            .await
            .unwrap()
            .unwrap();
        fs::write(&target, b"external\n").unwrap();
        drop(holder);
        assert_eq!(task.await.unwrap(), Err("SOURCE_VERSION_CONFLICT".into()));
        assert_eq!(fs::read(target).unwrap(), b"external\n");
    }

    /// pre-read 后的 Workspace generation 漂移必须在 commit lock 内 fail closed，原文件保持不变。
    #[tokio::test]
    async fn workspace_authority_drift_after_preread_does_not_commit() {
        let _guard = test_guard();
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("drift.txt");
        let existing = b"one\ntwo\n";
        fs::write(&target, existing).unwrap();
        let changed_supervisor = Arc::clone(&supervisor);
        let mut changed_workspace = workspace;
        changed_workspace.generation += 1;
        set_snapshot_ready_hook_for_test(Arc::new(move || {
            changed_supervisor
                .replace_workspaces(vec![changed_workspace.clone()])
                .unwrap()
        }));
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "drift.txt",
                expected(existing),
                1,
                1,
                "candidate",
                CancellationToken::new()
            )
            .await,
            Err("WORKSPACE_CHANGED".into())
        );
        assert_eq!(fs::read(target).unwrap(), existing);
    }

    /// pre-cancel 与同一 target lock wait cancel 都不得触碰 canonical target。
    #[tokio::test]
    async fn cancellation_before_and_while_waiting_lock_preserves_existing_file() {
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("held.txt");
        let existing = b"one\ntwo\n";
        fs::write(&target, existing).unwrap();
        let pre_cancel = CancellationToken::new();
        pre_cancel.cancel();
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "held.txt",
                expected(existing),
                1,
                1,
                "new",
                pre_cancel
            )
            .await,
            Err("CANCELLED".into())
        );
        let holder = existing_lock(supervisor.as_ref(), &workspace, "held.txt", existing).await;
        let cancel = CancellationToken::new();
        let waiting = replace(
            supervisor.as_ref(),
            "held.txt",
            expected(existing),
            1,
            1,
            "new",
            cancel.clone(),
        );
        tokio::pin!(waiting);
        tokio::task::yield_now().await;
        cancel.cancel();
        assert_eq!(waiting.await, Err("CANCELLED".into()));
        drop(holder);
        assert_eq!(fs::read(target).unwrap(), existing);
    }

    /// Windows 真实 junction 不能成为 replace target，shared resolver 必须在 snapshot 前拒绝。
    #[cfg(windows)]
    #[tokio::test]
    async fn rejects_actual_windows_junction_escape() {
        let (directory, supervisor, _workspace, root) = fixture();
        let outside = directory.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("target.txt"), b"outside\n").unwrap();
        let status = Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(root.join("escape"))
            .arg(&outside)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "escape/target.txt",
                expected(b"outside\n"),
                1,
                1,
                "inside",
                CancellationToken::new()
            )
            .await,
            Err("SOURCE_PATH_OUTSIDE_WORKSPACE".into())
        );
        assert_eq!(fs::read(outside.join("target.txt")).unwrap(), b"outside\n");
    }

    /// strict DTO 与 Remote registry/dispatch 均不能把本地 handler 暴露成 MCP Tool。
    #[tokio::test]
    async fn local_dto_is_strict_and_remote_does_not_advertise_or_dispatch_replace() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for arguments in [
            json!({"workspaceId":"workspace", "relativePath":"never.txt", "expectedSha256":"a".repeat(64), "startLine":1, "endLine":1, "content":"x"}),
            json!({"workspaceId":"workspace", "relative_path":"never.txt", "expectedSha256":"a".repeat(64), "startLine":1, "endLine":1, "content":"x", "root":"forbidden"}),
        ] {
            assert!(
                matches!(replace_lines(supervisor.as_ref(), arguments, CancellationToken::new()).await, Err(error) if error.starts_with("INVALID_PARAMS:"))
            );
        }
        assert!(
            !registry::list(false)
                .iter()
                .any(|tool| tool.name == "source_replace_lines")
        );
        assert_eq!(
            Broker::new(Arc::clone(&supervisor))
                .dispatch("source_replace_lines", json!({}), CancellationToken::new())
                .await,
            Err("UNKNOWN_TOOL".into())
        );
        assert_eq!(fs::read(root.join("inside.txt")).ok(), None);
    }

    /// P2C-004 的不确定 commit 状态必须穿透本 adapter 的唯一错误投影。
    #[test]
    fn commit_state_unknown_projection_is_preserved() {
        assert_eq!(
            source_error(SourceWriteError::CommitStateUnknown),
            "SOURCE_COMMIT_STATE_UNKNOWN"
        );
    }
}
