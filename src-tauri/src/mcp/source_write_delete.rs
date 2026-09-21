//! P2C-009 的本地按行删除 Source Write adapter；本模块绝不注册 Remote MCP Tool。
#![allow(
    dead_code,
    reason = "P2C-009 implements the local handler before a later task explicitly advertises Remote Source Write."
)]

use super::{
    registry,
    source_write_atomic_replace::replace_existing_file,
    source_write_commit::{TargetCommitError, lock_existing_target},
    source_write_domain::{
        SourceLineRange, SourceWriteError, SourceWriteSuccess, VersionedSourceWriteTarget,
        validate_result_text_file_size,
    },
    source_write_support::{
        ExistingTextSnapshotError, candidate_sha256, read_existing_text_snapshot,
        workspace_relative_path,
    },
};
use crate::{serena::SupervisorState, workspace_path::WorkspacePathResolver};
use serde::Deserialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// `source_delete_lines` 的严格本地输入；relative_path 保持冻结 snake_case wire 名。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceDeleteLinesInput {
    #[serde(flatten)]
    target: VersionedSourceWriteTarget,
    start_line: u32,
    end_line: u32,
}

/// 在显式 Workspace Lease 中按 1-based inclusive closed range 删除既有 UTF-8 文本行。
pub(crate) async fn delete_lines(
    supervisor: &SupervisorState,
    arguments: Value,
    cancel: CancellationToken,
) -> Result<SourceWriteSuccess, String> {
    // Workspace taxonomy 先由既有 adapter 统一判定，serde 不得改变 missing/null 的稳定分类。
    let workspace_id = registry::parse_workspace_id(&arguments)?;
    let input: SourceDeleteLinesInput =
        serde_json::from_value(arguments).map_err(|error| format!("INVALID_PARAMS: {error}"))?;
    debug_assert_eq!(input.target.workspace_id, workspace_id);
    delete_lines_input(supervisor, input, cancel).await
}

/// schema 解析后的 handler；range 与 version 均在 Guard 和 filesystem access 前完成纯校验。
async fn delete_lines_input(
    supervisor: &SupervisorState,
    input: SourceDeleteLinesInput,
    cancel: CancellationToken,
) -> Result<SourceWriteSuccess, String> {
    input.target.validate().map_err(source_error)?;
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

    // pre-read 只产生与 expected version 匹配的 bounded snapshot；删除 candidate 直接从其原始 bytes 边界切出。
    let snapshot = read_existing_text_snapshot(
        lease.clone(),
        input.target.relative_path.clone(),
        input.target.expected_sha256.clone(),
        cancel.clone(),
    )
    .await
    .map_err(snapshot_error)?;
    let candidate = delete_closed_line_range(&snapshot.text, changed_range)?;
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

/// 删除闭区间内每行及其 trailing line separator，保留其余区域的原始 bytes、mixed newline 与末尾换行。
pub(crate) fn delete_closed_line_range(
    existing: &str,
    range: SourceLineRange,
) -> Result<String, String> {
    range.validate().map_err(source_error)?;
    let line_count = u32::try_from(existing.lines().count())
        .map_err(|_| source_error(SourceWriteError::RangeInvalid))?;
    if range.end_line > line_count {
        return Err(source_error(SourceWriteError::RangeInvalid));
    }
    // endLine < N 时，下一个行首就是最后一个删除行 trailing separator 之后的 raw byte boundary。
    let delete_start = line_start_offset(existing, range.start_line);
    let delete_end = if range.end_line == line_count {
        existing.len()
    } else {
        line_start_offset(existing, range.end_line + 1)
    };
    let mut candidate = String::with_capacity(existing.len() - (delete_end - delete_start));
    candidate.push_str(&existing[..delete_start]);
    candidate.push_str(&existing[delete_end..]);
    Ok(candidate)
}

/// 返回既有 1-based 行号的 UTF-8 byte offset；调用方已验证行号位于 `1..=N`。
fn line_start_offset(existing: &str, line: u32) -> usize {
    if line == 1 {
        return 0;
    }
    existing
        .match_indices('\n')
        .nth((line - 2) as usize)
        .map(|(offset, _)| offset + 1)
        .expect("validated line must name an existing line")
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
            source_write_support::{
                candidate_sha256, set_snapshot_ready_hook_for_test, snapshot_hook_test_guard,
            },
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
    async fn delete(
        supervisor: &SupervisorState,
        relative_path: &str,
        expected_sha256: String,
        start_line: u32,
        end_line: u32,
        cancel: CancellationToken,
    ) -> Result<SourceWriteSuccess, String> {
        delete_lines(
            supervisor,
            json!({
                "workspaceId":"workspace",
                "relative_path":relative_path,
                "expectedSha256":expected_sha256,
                "startLine":start_line,
                "endLine":end_line,
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

    /// 删除中间闭区间、首行、尾行与单行均保留正确 bytes 和成功 provenance。
    #[tokio::test]
    async fn deletes_closed_ranges_with_provenance() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for (path, existing, start_line, end_line, expected_bytes) in [
            (
                "middle.txt",
                b"one\ntwo\nthree\nfour\nfive\nsix\n".as_slice(),
                3,
                5,
                b"one\ntwo\nsix\n".as_slice(),
            ),
            (
                "first.txt",
                b"one\ntwo\n".as_slice(),
                1,
                1,
                b"two\n".as_slice(),
            ),
            (
                "last.txt",
                b"one\ntwo\n".as_slice(),
                2,
                2,
                b"one\n".as_slice(),
            ),
            ("single.txt", b"only".as_slice(), 1, 1, b"".as_slice()),
        ] {
            fs::write(root.join(path), existing).unwrap();
            let result = delete(
                supervisor.as_ref(),
                path,
                expected(existing),
                start_line,
                end_line,
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
                    end_line,
                })
            );
            assert_eq!(result.changed_count, Some(end_line - start_line + 1));
        }
    }

    /// `delete_lines(1, N)` 产生精确空文件，而不是删除 target path 本身。
    #[tokio::test]
    async fn deletes_all_lines_to_an_existing_empty_file() {
        let (_directory, supervisor, _workspace, root) = fixture();
        let target = root.join("all.txt");
        let existing = b"one\r\ntwo\nthree";
        fs::write(&target, existing).unwrap();

        delete(
            supervisor.as_ref(),
            "all.txt",
            expected(existing),
            1,
            3,
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert!(target.is_file());
        assert_eq!(fs::read(target).unwrap(), b"");
    }

    /// LF、CRLF 与 mixed newline 仅移除目标行的 raw bytes 与其分隔符，未删除区域不得被重写。
    #[tokio::test]
    async fn preserves_lf_crlf_and_mixed_newline_bytes() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for (path, existing, start_line, end_line, expected_bytes) in [
            (
                "lf.txt",
                b"one\ntwo\nthree\n".as_slice(),
                2,
                2,
                b"one\nthree\n".as_slice(),
            ),
            (
                "crlf.txt",
                b"one\r\ntwo\r\nthree\r\n".as_slice(),
                2,
                2,
                b"one\r\nthree\r\n".as_slice(),
            ),
            (
                "mixed.txt",
                b"one\r\ntwo\nthree\r\nfour".as_slice(),
                2,
                3,
                b"one\r\nfour".as_slice(),
            ),
        ] {
            fs::write(root.join(path), existing).unwrap();
            delete(
                supervisor.as_ref(),
                path,
                expected(existing),
                start_line,
                end_line,
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
                delete(
                    supervisor.as_ref(),
                    "missing.txt",
                    expected(b"irrelevant"),
                    start_line,
                    end_line,
                    CancellationToken::new(),
                )
                .await,
                Err("SOURCE_RANGE_INVALID".into())
            );
        }
        assert!(!root.join("missing.txt").exists());
        let target = root.join("lines.txt");
        let existing = b"one\ntwo";
        fs::write(&target, existing).unwrap();
        assert_eq!(
            delete(
                supervisor.as_ref(),
                "lines.txt",
                expected(existing),
                1,
                3,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_RANGE_INVALID".into())
        );
        assert_eq!(fs::read(target).unwrap(), existing);
    }

    /// stale SHA、NUL/非 UTF-8 target 与 8 MiB target limit 均不得进入 replace。
    #[tokio::test]
    async fn rejects_stale_binary_and_oversized_targets_without_commit() {
        let (_directory, supervisor, _workspace, root) = fixture();
        let stale = root.join("stale.txt");
        fs::write(&stale, b"current\n").unwrap();
        assert_eq!(
            delete(
                supervisor.as_ref(),
                "stale.txt",
                expected(b"old\n"),
                1,
                1,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_VERSION_CONFLICT".into())
        );
        assert_eq!(fs::read(&stale).unwrap(), b"current\n");
        for (path, bytes) in [
            // Windows 将 NUL 及带扩展名的 NUL.* 保留为设备名，fixture 使用普通文件名。
            ("contains-nul.txt", b"one\0two\n".as_slice()),
            ("invalid-utf8.txt", b"one\xfftwo\n".as_slice()),
        ] {
            fs::write(root.join(path), bytes).unwrap();
            assert_eq!(
                delete(
                    supervisor.as_ref(),
                    path,
                    expected(bytes),
                    1,
                    1,
                    CancellationToken::new(),
                )
                .await,
                Err("SOURCE_BINARY_REJECTED".into())
            );
            assert_eq!(fs::read(root.join(path)).unwrap(), bytes);
        }
        let oversized = vec![b'x'; 8 * 1024 * 1024 + 1];
        fs::write(root.join("oversized.txt"), &oversized).unwrap();
        assert_eq!(
            delete(
                supervisor.as_ref(),
                "oversized.txt",
                expected(&oversized),
                1,
                1,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_FILE_TOO_LARGE".into())
        );
    }

    /// pre-read 后的 external edit 必须由 P2C-003 locked revalidation 拒绝，不能覆盖外部 bytes。
    #[tokio::test]
    async fn external_edit_after_preread_is_not_overwritten() {
        let _test_guard = snapshot_hook_test_guard();
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("race.txt");
        fs::write(&target, b"one\ntwo\n").unwrap();
        let holder =
            existing_lock(supervisor.as_ref(), &workspace, "race.txt", b"one\ntwo\n").await;
        let (ready_tx, ready_rx) = mpsc::channel();
        set_snapshot_ready_hook_for_test(&target, Arc::new(move || ready_tx.send(()).unwrap()));
        let task_supervisor = Arc::clone(&supervisor);
        let task = tokio::spawn(async move {
            delete(
                task_supervisor.as_ref(),
                "race.txt",
                expected(b"one\ntwo\n"),
                1,
                1,
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
        let _test_guard = snapshot_hook_test_guard();
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("drift.txt");
        let existing = b"one\ntwo\n";
        fs::write(&target, existing).unwrap();
        let changed_supervisor = Arc::clone(&supervisor);
        let mut changed_workspace = workspace;
        changed_workspace.generation += 1;
        set_snapshot_ready_hook_for_test(
            &target,
            Arc::new(move || {
                changed_supervisor
                    .replace_workspaces(vec![changed_workspace.clone()])
                    .unwrap();
            }),
        );
        assert_eq!(
            delete(
                supervisor.as_ref(),
                "drift.txt",
                expected(existing),
                1,
                1,
                CancellationToken::new(),
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
            delete(
                supervisor.as_ref(),
                "held.txt",
                expected(existing),
                1,
                1,
                pre_cancel,
            )
            .await,
            Err("CANCELLED".into())
        );
        let holder = existing_lock(supervisor.as_ref(), &workspace, "held.txt", existing).await;
        let cancel = CancellationToken::new();
        let waiting = delete(
            supervisor.as_ref(),
            "held.txt",
            expected(existing),
            1,
            1,
            cancel.clone(),
        );
        tokio::pin!(waiting);
        tokio::task::yield_now().await;
        cancel.cancel();
        assert_eq!(waiting.await, Err("CANCELLED".into()));
        drop(holder);
        assert_eq!(fs::read(target).unwrap(), existing);
    }

    /// Windows 真实 junction 不能成为 delete target，shared resolver 必须在 snapshot 前拒绝。
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
            delete(
                supervisor.as_ref(),
                "escape/target.txt",
                expected(b"outside\n"),
                1,
                1,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_PATH_OUTSIDE_WORKSPACE".into())
        );
        assert_eq!(fs::read(outside.join("target.txt")).unwrap(), b"outside\n");
    }

    /// strict DTO 与 Remote registry/dispatch 均不能把本地 handler 暴露成 MCP Tool。
    #[tokio::test]
    async fn local_dto_is_strict_and_remote_does_not_advertise_or_dispatch_delete() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for arguments in [
            json!({"workspaceId":"workspace", "relativePath":"never.txt", "expectedSha256":"a".repeat(64), "startLine":1, "endLine":1}),
            json!({"workspaceId":"workspace", "relative_path":"never.txt", "expectedSha256":"a".repeat(64), "startLine":1, "endLine":1, "root":"forbidden"}),
        ] {
            assert!(matches!(
                delete_lines(supervisor.as_ref(), arguments, CancellationToken::new()).await,
                Err(error) if error.starts_with("INVALID_PARAMS:")
            ));
        }
        assert!(
            !registry::list(false)
                .iter()
                .any(|tool| tool.name == "source_delete_lines")
        );
        assert_eq!(
            Broker::new(Arc::clone(&supervisor))
                .dispatch("source_delete_lines", json!({}), CancellationToken::new())
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
