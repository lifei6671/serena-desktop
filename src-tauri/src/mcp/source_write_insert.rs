//! P2C-008 的本地按行插入 Source Write adapter；本模块绝不注册 Remote MCP Tool。
#![allow(
    dead_code,
    reason = "P2C-008 implements the local handler before a later task explicitly advertises Remote Source Write."
)]

use super::{
    registry,
    source_write_atomic_replace::replace_existing_file,
    source_write_commit::{TargetCommitError, lock_existing_target},
    source_write_domain::{
        SourceLineRange, SourceWriteError, SourceWriteSuccess, VersionedSourceWriteTarget,
        validate_inline_mutation_content, validate_result_text_file_size,
    },
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

/// `source_insert_lines` 的严格本地输入；relative_path 保持冻结 snake_case wire 名。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceInsertLinesInput {
    #[serde(flatten)]
    target: VersionedSourceWriteTarget,
    before_line: u32,
    content: String,
}

/// 在显式 Workspace Lease 中按 1-based beforeLine 插入 UTF-8 文本行。
pub(crate) async fn insert_lines(
    supervisor: &SupervisorState,
    arguments: Value,
    cancel: CancellationToken,
) -> Result<SourceWriteSuccess, String> {
    // Workspace taxonomy 先由既有 adapter 统一判定，serde 不得改变 missing/null 的稳定分类。
    let workspace_id = registry::parse_workspace_id(&arguments)?;
    let input: SourceInsertLinesInput =
        serde_json::from_value(arguments).map_err(|error| format!("INVALID_PARAMS: {error}"))?;
    debug_assert_eq!(input.target.workspace_id, workspace_id);
    insert_lines_input(supervisor, input, cancel).await
}

/// schema 解析后的 handler；正文、行号和版本均在 Guard 与 filesystem access 前完成纯校验。
async fn insert_lines_input(
    supervisor: &SupervisorState,
    input: SourceInsertLinesInput,
    cancel: CancellationToken,
) -> Result<SourceWriteSuccess, String> {
    input.target.validate().map_err(source_error)?;
    if input.content.is_empty() {
        return Err(source_error(SourceWriteError::InvalidArgument));
    }
    validate_inline_mutation_content(&input.content).map_err(source_error)?;
    // 0 不依赖目标文件的行数，必须在取得任何 Workspace 或文件 authority 前稳定拒绝。
    if input.before_line == 0 {
        return Err(source_error(SourceWriteError::RangeInvalid));
    }
    if cancel.is_cancelled() {
        return Err("CANCELLED".into());
    }

    // 只从 caller 的显式 workspaceId 取得 captured Lease 与 RAII Guard，绝不回退到 UI 或 session state。
    let (lease, guard) = supervisor.resolve_workspace_write_guard(&input.target.workspace_id)?;
    WorkspacePathResolver::new(&lease)
        .resolve(&input.target.relative_path)
        .map_err(|_| source_error(SourceWriteError::PathOutsideWorkspace))?;

    // pre-read 只产生与 expected version 匹配的 bounded snapshot，用其 newline policy 生成 candidate。
    let snapshot = read_existing_text_snapshot(
        lease.clone(),
        input.target.relative_path.clone(),
        input.target.expected_sha256.clone(),
        cancel.clone(),
    )
    .await
    .map_err(snapshot_error)?;
    let newline_style = detect_newline_style(&snapshot.text);
    let normalized = normalize_newlines(&input.content, newline_style);
    let (candidate, inserted_count) = insert_normalized_lines(
        &snapshot.text,
        &normalized,
        input.before_line,
        newline_style,
    )?;
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
        changed_range: Some(SourceLineRange {
            start_line: input.before_line,
            end_line: input.before_line + inserted_count - 1,
        }),
        changed_count: Some(inserted_count),
    };
    success.validate().map_err(source_error)?;
    Ok(success)
}

/// 在文本行边界插入已规范化正文，保留原文件的 bytes、final newline 与 mixed newline 内容。
pub(crate) fn insert_normalized_lines(
    existing: &str,
    content: &str,
    before_line: u32,
    newline_style: super::source_write_text::NewlineStyle,
) -> Result<(String, u32), String> {
    let existing_count = existing.lines().count();
    let maximum_before_line = existing_count
        .checked_add(1)
        .and_then(|count| u32::try_from(count).ok())
        .ok_or_else(|| source_error(SourceWriteError::RangeInvalid))?;
    if before_line == 0 || before_line > maximum_before_line {
        return Err(source_error(SourceWriteError::RangeInvalid));
    }
    let inserted_count = u32::try_from(content.lines().count())
        .ok()
        .filter(|count| *count > 0)
        .ok_or_else(|| source_error(SourceWriteError::InvalidArgument))?;
    let insertion_offset = line_start_offset(existing, before_line, maximum_before_line);
    let newline = normalize_newlines("\n", newline_style);
    let mut candidate = String::with_capacity(existing.len() + content.len() + newline.len() * 2);
    let prefix = &existing[..insertion_offset];
    let suffix = &existing[insertion_offset..];
    candidate.push_str(prefix);
    if !prefix.is_empty() && !prefix.ends_with('\n') {
        candidate.push_str(&newline);
    }
    candidate.push_str(content);
    if !suffix.is_empty() && !content.ends_with('\n') {
        candidate.push_str(&newline);
    }
    candidate.push_str(suffix);
    Ok((candidate, inserted_count))
}

/// 返回 1-based beforeLine 的 UTF-8 byte offset；N+1 固定指向文件末尾。
fn line_start_offset(existing: &str, before_line: u32, append_line: u32) -> usize {
    if before_line == append_line {
        return existing.len();
    }
    if before_line == 1 {
        return 0;
    }
    existing
        .match_indices('\n')
        .nth((before_line - 2) as usize)
        .map(|(offset, _)| offset + 1)
        .expect("validated beforeLine must name an existing line")
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
        sync::{Arc, mpsc},
        time::Duration,
    };

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
    async fn insert(
        supervisor: &SupervisorState,
        relative_path: &str,
        expected_sha256: String,
        before_line: u32,
        content: &str,
        cancel: CancellationToken,
    ) -> Result<SourceWriteSuccess, String> {
        insert_lines(
            supervisor,
            json!({
                "workspaceId":"workspace",
                "relative_path":relative_path,
                "expectedSha256":expected_sha256,
                "beforeLine":before_line,
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

    /// 首部、中间与 N+1 append 都产生正确行边界与成功 provenance。
    #[tokio::test]
    async fn inserts_at_first_middle_and_append_with_provenance() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for (path, existing, before_line, content, expected_bytes, range, count) in [
            (
                "first.txt",
                b"second\nthird\n".as_slice(),
                1,
                "first",
                b"first\nsecond\nthird\n".as_slice(),
                (1, 1),
                1,
            ),
            (
                "middle.txt",
                b"one\ntwo\nthree".as_slice(),
                2,
                "middle\nline",
                b"one\nmiddle\nline\ntwo\nthree".as_slice(),
                (2, 3),
                2,
            ),
            (
                "append.txt",
                b"one\ntwo".as_slice(),
                3,
                "three",
                b"one\ntwo\nthree".as_slice(),
                (3, 3),
                1,
            ),
        ] {
            fs::write(root.join(path), existing).unwrap();
            let result = insert(
                supervisor.as_ref(),
                path,
                expected(existing),
                before_line,
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
                    start_line: range.0,
                    end_line: range.1,
                })
            );
            assert_eq!(result.changed_count, Some(count));
        }
    }

    /// 单行、空文件、尾换行和 multi-line content 都遵守 `.lines()` 的冻结行计数语义。
    #[tokio::test]
    async fn inserts_on_single_and_empty_file_boundaries() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for (path, existing, before_line, content, expected_bytes, count) in [
            (
                "single-first.txt",
                b"only".as_slice(),
                1,
                "first",
                b"first\nonly".as_slice(),
                1,
            ),
            (
                "single-append.txt",
                b"only".as_slice(),
                2,
                "last\n",
                b"only\nlast\n".as_slice(),
                1,
            ),
            (
                "empty.txt",
                b"".as_slice(),
                1,
                "first\nsecond",
                b"first\nsecond".as_slice(),
                2,
            ),
        ] {
            fs::write(root.join(path), existing).unwrap();
            let result = insert(
                supervisor.as_ref(),
                path,
                expected(existing),
                before_line,
                content,
                CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(fs::read(root.join(path)).unwrap(), expected_bytes);
            assert_eq!(result.changed_count, Some(count));
        }
    }

    /// 0、超过 N+1 与空正文在 replace 前以各自冻结 taxonomy 失败，并保留原 bytes。
    #[tokio::test]
    async fn rejects_invalid_ranges_and_empty_content_without_commit() {
        let (_directory, supervisor, _workspace, root) = fixture();
        assert_eq!(
            insert(
                supervisor.as_ref(),
                "missing.txt",
                expected(b"not read"),
                0,
                "new",
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_RANGE_INVALID".into())
        );
        assert!(!root.join("missing.txt").exists());
        let target = root.join("lines.txt");
        let existing = b"one\ntwo";
        fs::write(&target, existing).unwrap();
        for (before_line, content, error) in [
            (0, "new", "SOURCE_RANGE_INVALID"),
            (4, "new", "SOURCE_RANGE_INVALID"),
            (1, "", "SOURCE_INVALID_ARGUMENT"),
        ] {
            assert_eq!(
                insert(
                    supervisor.as_ref(),
                    "lines.txt",
                    expected(existing),
                    before_line,
                    content,
                    CancellationToken::new(),
                )
                .await,
                Err(error.into())
            );
            assert_eq!(fs::read(&target).unwrap(), existing);
        }
    }

    /// LF、CRLF 与 mixed tie-break 均只规范化插入正文和新增的行分隔符，不重写原有 mixed bytes。
    #[tokio::test]
    async fn preserves_target_newline_policy_for_lf_crlf_and_mixed_tie() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for (path, existing, expected_bytes) in [
            (
                "lf.txt",
                b"one\ntwo".as_slice(),
                b"one\nfirst\nsecond\ntwo".as_slice(),
            ),
            (
                "crlf.txt",
                b"one\r\ntwo".as_slice(),
                b"one\r\nfirst\r\nsecond\r\ntwo".as_slice(),
            ),
            (
                "tie.txt",
                b"one\r\ntwo\nthree".as_slice(),
                b"one\r\nfirst\r\nsecond\r\ntwo\nthree".as_slice(),
            ),
        ] {
            fs::write(root.join(path), existing).unwrap();
            insert(
                supervisor.as_ref(),
                path,
                expected(existing),
                2,
                "first\nsecond\r\n",
                CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(fs::read(root.join(path)).unwrap(), expected_bytes);
        }
    }

    /// stale expected、NUL、inline input、target 与 result hard limit 各自保持冻结错误码。
    #[tokio::test]
    async fn rejects_stale_binary_and_all_insert_size_limits() {
        let (_directory, supervisor, _workspace, root) = fixture();
        fs::write(root.join("stale.txt"), b"current").unwrap();
        assert_eq!(
            insert(
                supervisor.as_ref(),
                "stale.txt",
                expected(b"old"),
                1,
                "new",
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_VERSION_CONFLICT".into())
        );
        assert_eq!(
            insert(
                supervisor.as_ref(),
                "stale.txt",
                expected(b"current"),
                1,
                "new\0text",
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_BINARY_REJECTED".into())
        );
        assert_eq!(
            insert(
                supervisor.as_ref(),
                "stale.txt",
                expected(b"current"),
                1,
                &"x".repeat(1024 * 1024 + 1),
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_INPUT_LIMIT_EXCEEDED".into())
        );

        let too_large_target = vec![b'x'; 8 * 1024 * 1024 + 1];
        fs::write(root.join("too-large-target.txt"), &too_large_target).unwrap();
        assert_eq!(
            insert(
                supervisor.as_ref(),
                "too-large-target.txt",
                expected(&too_large_target),
                1,
                "new",
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_FILE_TOO_LARGE".into())
        );

        let nearly_full = vec![b'x'; 8 * 1024 * 1024 - 1];
        fs::write(root.join("too-large-result.txt"), &nearly_full).unwrap();
        assert_eq!(
            insert(
                supervisor.as_ref(),
                "too-large-result.txt",
                expected(&nearly_full),
                2,
                "xx",
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_FILE_TOO_LARGE".into())
        );
    }

    /// pre-read 后的 external edit 必须由 P2C-003 locked revalidation 拒绝，不能覆盖外部 bytes。
    #[tokio::test]
    async fn external_edit_after_preread_is_not_overwritten() {
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("race.txt");
        fs::write(&target, b"old\n").unwrap();
        let holder = existing_lock(supervisor.as_ref(), &workspace, "race.txt", b"old\n").await;
        let (ready_tx, ready_rx) = mpsc::channel();
        set_snapshot_ready_hook_for_test(Arc::new(move || ready_tx.send(()).unwrap()));
        let task_supervisor = Arc::clone(&supervisor);
        let task = tokio::spawn(async move {
            insert(
                task_supervisor.as_ref(),
                "race.txt",
                expected(b"old\n"),
                2,
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
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("drift.txt");
        fs::write(&target, b"old\n").unwrap();
        let changed_supervisor = Arc::clone(&supervisor);
        let mut changed_workspace = workspace;
        changed_workspace.generation += 1;
        set_snapshot_ready_hook_for_test(Arc::new(move || {
            changed_supervisor
                .replace_workspaces(vec![changed_workspace.clone()])
                .unwrap();
        }));
        assert_eq!(
            insert(
                supervisor.as_ref(),
                "drift.txt",
                expected(b"old\n"),
                2,
                "candidate",
                CancellationToken::new(),
            )
            .await,
            Err("WORKSPACE_CHANGED".into())
        );
        assert_eq!(fs::read(target).unwrap(), b"old\n");
    }

    /// pre-cancel 与同一 target lock wait cancel 都不得触碰 canonical target。
    #[tokio::test]
    async fn cancellation_before_and_while_waiting_lock_preserves_existing_file() {
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("held.txt");
        fs::write(&target, b"old\n").unwrap();
        let pre_cancel = CancellationToken::new();
        pre_cancel.cancel();
        assert_eq!(
            insert(
                supervisor.as_ref(),
                "held.txt",
                expected(b"old\n"),
                2,
                "new",
                pre_cancel,
            )
            .await,
            Err("CANCELLED".into())
        );

        let holder = existing_lock(supervisor.as_ref(), &workspace, "held.txt", b"old\n").await;
        let cancel = CancellationToken::new();
        let waiting = insert(
            supervisor.as_ref(),
            "held.txt",
            expected(b"old\n"),
            2,
            "new",
            cancel.clone(),
        );
        tokio::pin!(waiting);
        tokio::task::yield_now().await;
        cancel.cancel();
        assert_eq!(waiting.await, Err("CANCELLED".into()));
        drop(holder);
        assert_eq!(fs::read(target).unwrap(), b"old\n");
    }

    /// 显式未知 Workspace 与 escape path 都拒绝；绝不回退到任何 active Workspace。
    #[tokio::test]
    async fn rejects_unknown_workspace_and_workspace_escape() {
        let (_directory, supervisor, _workspace, root) = fixture();
        fs::write(root.join("inside.txt"), b"inside\n").unwrap();
        assert_eq!(
            insert_lines(
                supervisor.as_ref(),
                json!({
                    "workspaceId":"missing",
                    "relative_path":"inside.txt",
                    "expectedSha256":expected(b"inside\n"),
                    "beforeLine":1,
                    "content":"new",
                }),
                CancellationToken::new(),
            )
            .await,
            Err("WORKSPACE_NOT_FOUND".into())
        );
        assert_eq!(
            insert(
                supervisor.as_ref(),
                "../outside.txt",
                expected(b"irrelevant"),
                1,
                "new",
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_PATH_OUTSIDE_WORKSPACE".into())
        );
    }

    /// Windows 真实 junction 不能成为 insert target，shared resolver 必须在 snapshot 前拒绝。
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
            insert(
                supervisor.as_ref(),
                "escape/target.txt",
                expected(b"outside\n"),
                1,
                "new",
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_PATH_OUTSIDE_WORKSPACE".into())
        );
        assert_eq!(fs::read(outside.join("target.txt")).unwrap(), b"outside\n");
    }

    /// strict DTO 与 Remote registry/dispatch 均不能把本地 handler 暴露成 MCP Tool。
    #[tokio::test]
    async fn local_dto_is_strict_and_remote_does_not_advertise_or_dispatch_insert() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for arguments in [
            json!({"workspaceId":"workspace", "relativePath":"never.txt", "expectedSha256":"a".repeat(64), "beforeLine":1, "content":"x"}),
            json!({"workspaceId":"workspace", "relative_path":"never.txt", "expectedSha256":"a".repeat(64), "beforeLine":1, "content":"x", "root":"forbidden"}),
        ] {
            assert!(matches!(
                insert_lines(supervisor.as_ref(), arguments, CancellationToken::new()).await,
                Err(error) if error.starts_with("INVALID_PARAMS:")
            ));
        }
        assert!(
            !registry::list(false)
                .iter()
                .any(|tool| tool.name == "source_insert_lines")
        );
        assert_eq!(
            Broker::new(Arc::clone(&supervisor))
                .dispatch("source_insert_lines", json!({}), CancellationToken::new(),)
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
