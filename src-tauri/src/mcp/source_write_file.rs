//! P2C-007 的本地整文件 Source Write adapter；本模块绝不注册 Remote MCP Tool。
#![allow(
    dead_code,
    reason = "P2C-007 implements the local handler before a later task explicitly advertises Remote Source Write."
)]

use super::{
    registry,
    source_write_atomic_replace::{CreateNewFileError, create_new_file, replace_existing_file},
    source_write_commit::{TargetCommitError, lock_existing_target, lock_vacant_target},
    source_write_domain::{
        ExpectedSha256, SourceWriteError, SourceWriteSuccess, SourceWriteTarget,
        validate_result_text_file_size, validate_whole_file_write_content,
    },
    source_write_support::{
        ExistingTextSnapshotError, candidate_sha256, read_existing_text_snapshot,
        workspace_relative_path,
    },
    source_write_text::{NewlineStyle, detect_newline_style, normalize_newlines},
};
use crate::{serena::SupervisorState, workspace_path::WorkspacePathResolver};
use serde::Deserialize;
use serde_json::Value;
#[cfg(test)]
use std::sync::{Arc, Mutex, OnceLock};
use tokio_util::sync::CancellationToken;

/// 仅测试在 existing lock 成功后、replace 前精确注入取消，不进入 production 构建。
#[cfg(test)]
type AfterExistingLockHook = Arc<dyn Fn() + Send + Sync>;
#[cfg(test)]
static AFTER_EXISTING_LOCK_HOOK: OnceLock<Mutex<Option<AfterExistingLockHook>>> = OnceLock::new();

/// 设置一次性 existing-lock hook，确保 cancellation regression 覆盖 replace 前的唯一窗口。
#[cfg(test)]
fn set_after_existing_lock_hook_for_test(hook: AfterExistingLockHook) {
    *AFTER_EXISTING_LOCK_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some(hook);
}

/// 取走并调用一次 hook，不允许测试注入进入 production handler 语义。
#[cfg(test)]
fn run_after_existing_lock_hook_for_test() {
    if let Some(hook) = AFTER_EXISTING_LOCK_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .take()
    {
        hook();
    }
}

/// `ifExists` 的唯一强类型 wire 枚举；不接受 create/replace 等非冻结别名。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum IfExists {
    Fail,
    Overwrite,
}

/// `source_write_text_file` 的严格本地输入；relative_path 保持冻结 snake_case wire 名。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceWriteTextFileInput {
    workspace_id: String,
    #[serde(rename = "relative_path")]
    relative_path: String,
    content: String,
    if_exists: IfExists,
    expected_sha256: Option<ExpectedSha256>,
}

/// 在显式 Workspace Lease 中创建或按 expected SHA 原子替换完整 UTF-8 文本文件。
pub(crate) async fn write_text_file(
    supervisor: &SupervisorState,
    arguments: Value,
    cancel: CancellationToken,
) -> Result<SourceWriteSuccess, String> {
    // Workspace taxonomy 先由既有 adapter 统一判定，serde 不得改变 missing/null 的稳定分类。
    let workspace_id = registry::parse_workspace_id(&arguments)?;
    let input: SourceWriteTextFileInput =
        serde_json::from_value(arguments).map_err(|error| format!("INVALID_PARAMS: {error}"))?;
    debug_assert_eq!(input.workspace_id, workspace_id);
    write_text_file_input(supervisor, input, cancel).await
}

/// schema 解析后的 handler；所有 caller content 纯校验都在 Guard 与 filesystem access 之前完成。
async fn write_text_file_input(
    supervisor: &SupervisorState,
    input: SourceWriteTextFileInput,
    cancel: CancellationToken,
) -> Result<SourceWriteSuccess, String> {
    let target = SourceWriteTarget {
        workspace_id: input.workspace_id,
        relative_path: input.relative_path,
    };
    target.validate().map_err(source_error)?;
    validate_whole_file_write_content(&input.content).map_err(source_error)?;
    if cancel.is_cancelled() {
        return Err("CANCELLED".into());
    }

    // 只从 caller 的显式 workspaceId 取得 captured Lease 与 RAII Guard，绝不回退到 UI 或 session state。
    let (lease, guard) = supervisor.resolve_workspace_write_guard(&target.workspace_id)?;
    let canonical_target = WorkspacePathResolver::new(&lease)
        .resolve(&target.relative_path)
        .map_err(|_| source_error(SourceWriteError::PathOutsideWorkspace))?;

    if !canonical_target.exists() {
        return create_absent_target(supervisor, &target, lease, guard, &input.content, cancel)
            .await;
    }
    if input.if_exists == IfExists::Fail {
        return Err(source_error(SourceWriteError::AlreadyExists));
    }
    let expected_sha256 = input
        .expected_sha256
        .ok_or_else(|| source_error(SourceWriteError::VersionRequired))?;

    // pre-read 只产生与 expected version 匹配的 bounded snapshot，用其 newline policy 生成 candidate。
    let snapshot = read_existing_text_snapshot(
        lease.clone(),
        target.relative_path.clone(),
        expected_sha256.clone(),
        cancel.clone(),
    )
    .await
    .map_err(snapshot_error)?;
    let normalized = normalize_newlines(&input.content, detect_newline_style(&snapshot.text));
    validate_result_text_file_size(normalized.len()).map_err(source_error)?;
    let candidate = normalized.into_bytes();
    let after_sha256 = candidate_sha256(&candidate);

    if cancel.is_cancelled() {
        return Err("CANCELLED".into());
    }
    // 同一 Lease 与 Guard 进入 P2C-003；锁内将再次验证 expected SHA，闭合 pre-read 到 replace 的 OCC。
    let locked = tokio::select! {
        _ = cancel.cancelled() => return Err("CANCELLED".into()),
        result = lock_existing_target(supervisor, lease.clone(), guard, &target.relative_path, expected_sha256) => {
            result.map_err(target_commit_error)?
        }
    };
    #[cfg(test)]
    run_after_existing_lock_hook_for_test();
    if cancel.is_cancelled() {
        return Err("CANCELLED".into());
    }
    let before_sha256 = locked
        .before_sha256()
        .cloned()
        .ok_or_else(|| source_error(SourceWriteError::IoError))?;
    let path = workspace_relative_path(&lease.canonical_root, locked.canonical_target())?;

    // 一旦进入 P2C-004 replace，不再观察 cancellation；UNKNOWN 必须原样向上投影。
    replace_existing_file(&locked, &candidate).map_err(source_error)?;
    let success = SourceWriteSuccess {
        path,
        workspace_id: lease.workspace_id,
        generation: lease.generation,
        before_sha256: Some(before_sha256),
        after_sha256,
        changed_range: None,
        changed_count: None,
    };
    success.validate().map_err(source_error)?;
    Ok(success)
}

/// target 不存在时无视 ifExists 与 optional expected SHA，固定 LF 后复用 P2C-006 no-clobber create primitive。
async fn create_absent_target(
    supervisor: &SupervisorState,
    target: &SourceWriteTarget,
    lease: crate::workspace_resolver::WorkspaceLease,
    guard: crate::serena::WorkspaceWriteGuard,
    content: &str,
    cancel: CancellationToken,
) -> Result<SourceWriteSuccess, String> {
    let normalized = normalize_newlines(content, NewlineStyle::Lf);
    validate_result_text_file_size(normalized.len()).map_err(source_error)?;
    let candidate = normalized.into_bytes();
    let after_sha256 = candidate_sha256(&candidate);
    let locked = tokio::select! {
        _ = cancel.cancelled() => return Err("CANCELLED".into()),
        result = lock_vacant_target(supervisor, lease.clone(), guard, &target.relative_path) => {
            result.map_err(target_commit_error)?
        }
    };
    if cancel.is_cancelled() {
        return Err("CANCELLED".into());
    }
    let path = workspace_relative_path(&lease.canonical_root, locked.canonical_target())?;
    match create_new_file(&locked, &candidate, &cancel) {
        Ok(()) => {}
        Err(CreateNewFileError::Cancelled) => return Err("CANCELLED".into()),
        Err(CreateNewFileError::Source(error)) => return Err(source_error(error)),
    }
    let success = SourceWriteSuccess {
        path,
        workspace_id: lease.workspace_id,
        generation: lease.generation,
        before_sha256: None,
        after_sha256,
        changed_range: None,
        changed_count: None,
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
            registry,
            source_write_commit::{LockedTargetCommit, lock_existing_target},
            source_write_support::{
                candidate_sha256, set_snapshot_before_resolve_hook_for_test,
                set_snapshot_chunk_hook_for_test, set_snapshot_ready_hook_for_test,
            },
        },
    };
    use serde_json::json;
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::{Arc, Mutex, MutexGuard, OnceLock, mpsc},
        time::Duration,
    };

    /// snapshot/lock hook 是 process-global test seam；串行化本模块测试避免互相消费一次性 hook。
    static TEST_SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

    /// 获取 P2C-007 handler 测试的唯一 hook ownership，释放后才允许下一条测试安装 hook。
    fn test_guard() -> MutexGuard<'static, ()> {
        TEST_SERIAL.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    /// 创建未被 Desktop selection 或 legacy active state 影响的最小已注册 Workspace。
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

    /// 以冻结 DTO 调用 local handler；它不经过 Remote registry 或 Broker dispatch。
    async fn write(
        supervisor: &SupervisorState,
        relative_path: &str,
        content: &str,
        if_exists: &str,
        expected_sha256: Option<String>,
        cancel: CancellationToken,
    ) -> Result<SourceWriteSuccess, String> {
        let mut arguments = json!({
            "workspaceId":"workspace",
            "relative_path":relative_path,
            "content":content,
            "ifExists":if_exists,
        });
        if let Some(expected_sha256) = expected_sha256 {
            arguments["expectedSha256"] = json!(expected_sha256);
        }
        write_text_file(supervisor, arguments, cancel).await
    }

    /// 从既有 raw bytes 生成 handler 应接受的冻结 expected version token。
    fn expected(bytes: &[u8]) -> String {
        candidate_sha256(bytes).as_str().into()
    }

    /// 直接取得 existing lock，供 handler lock-wait cancellation 与 OCC 测试复用。
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

    /// absent target 无论 fail/overwrite 都以 LF 创建，optional expectedSha256 不改变 create 语义。
    #[tokio::test]
    async fn absent_targets_create_with_lf_for_both_if_exists_values() {
        let _test_guard = test_guard();
        let (_directory, supervisor, _workspace, root) = fixture();
        for (path, if_exists, provided_expected) in [
            ("fail.txt", "fail", None),
            ("overwrite.txt", "overwrite", Some("a".repeat(64))),
        ] {
            let result = write(
                supervisor.as_ref(),
                path,
                "one\r\ntwo\nthree\rfour",
                if_exists,
                provided_expected,
                CancellationToken::new(),
            )
            .await
            .unwrap();
            let bytes = b"one\ntwo\nthree\rfour";
            assert_eq!(fs::read(root.join(path)).unwrap(), bytes);
            assert!(result.before_sha256.is_none());
            assert_eq!(result.after_sha256.as_str(), expected(bytes));
            assert_eq!(result.path, path);
            assert_eq!(result.workspace_id, "workspace");
            assert_eq!(result.generation, 7);
            assert!(result.changed_range.is_none());
            assert!(result.changed_count.is_none());
        }
    }

    /// existing fail 与 overwrite 缺少 version 都不能读取或改变现有 bytes。
    #[tokio::test]
    async fn existing_fail_and_missing_version_preserve_bytes() {
        let _test_guard = test_guard();
        let (_directory, supervisor, _workspace, root) = fixture();
        let target = root.join("existing.txt");
        fs::write(&target, b"old\r\ntext\r\n").unwrap();
        for (if_exists, expected_sha256, error) in [
            ("fail", None, "SOURCE_ALREADY_EXISTS"),
            ("overwrite", None, "SOURCE_VERSION_REQUIRED"),
        ] {
            assert_eq!(
                write(
                    supervisor.as_ref(),
                    "existing.txt",
                    "new",
                    if_exists,
                    expected_sha256,
                    CancellationToken::new(),
                )
                .await,
                Err(error.into())
            );
            assert_eq!(fs::read(&target).unwrap(), b"old\r\ntext\r\n");
        }
    }

    /// overwrite 保留 verified existing text 的 LF、CRLF 与 mixed/tie newline policy。
    #[tokio::test]
    async fn overwrite_preserves_verified_dominant_newline_style() {
        let _test_guard = test_guard();
        let (_directory, supervisor, _workspace, root) = fixture();
        for (path, existing, expected_new) in [
            (
                "lf.txt",
                b"old\nline\n".as_slice(),
                b"new\nline\n".as_slice(),
            ),
            (
                "crlf.txt",
                b"old\r\nline\r\n".as_slice(),
                b"new\r\nline\r\n".as_slice(),
            ),
            (
                "tie.txt",
                b"old\r\nline\n".as_slice(),
                b"new\r\nline\r\n".as_slice(),
            ),
        ] {
            let target = root.join(path);
            fs::write(&target, existing).unwrap();
            let result = write(
                supervisor.as_ref(),
                path,
                "new\nline\r\n",
                "overwrite",
                Some(expected(existing)),
                CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(fs::read(target).unwrap(), expected_new);
            assert_eq!(result.before_sha256.unwrap().as_str(), expected(existing));
            assert_eq!(result.after_sha256.as_str(), expected(expected_new));
        }
    }

    /// stale expected、binary target、caller NUL、input/target/result hard limits保持各自冻结 taxonomy。
    #[tokio::test]
    async fn rejects_versions_binary_data_and_all_whole_file_limits() {
        let _test_guard = test_guard();
        let (_directory, supervisor, _workspace, root) = fixture();
        fs::write(root.join("stale.txt"), b"current").unwrap();
        assert_eq!(
            write(
                supervisor.as_ref(),
                "stale.txt",
                "new",
                "overwrite",
                Some(expected(b"old")),
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_VERSION_CONFLICT".into())
        );
        assert_eq!(fs::read(root.join("stale.txt")).unwrap(), b"current");

        fs::write(root.join("binary.txt"), b"binary\0target").unwrap();
        assert_eq!(
            write(
                supervisor.as_ref(),
                "binary.txt",
                "new",
                "overwrite",
                Some(expected(b"binary\0target")),
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_BINARY_REJECTED".into())
        );
        assert_eq!(
            write(
                supervisor.as_ref(),
                "new.txt",
                "caller\0nul",
                "fail",
                None,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_BINARY_REJECTED".into())
        );
        assert_eq!(
            write(
                supervisor.as_ref(),
                "too-large-input.txt",
                &"x".repeat(8 * 1024 * 1024 + 1),
                "fail",
                None,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_INPUT_LIMIT_EXCEEDED".into())
        );

        let too_large_target = vec![b'x'; 8 * 1024 * 1024 + 1];
        fs::write(root.join("too-large-target.txt"), &too_large_target).unwrap();
        assert_eq!(
            write(
                supervisor.as_ref(),
                "too-large-target.txt",
                "new",
                "overwrite",
                Some(expected(&too_large_target)),
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_FILE_TOO_LARGE".into())
        );

        fs::write(root.join("crlf-result.txt"), b"old\r\n").unwrap();
        assert_eq!(
            write(
                supervisor.as_ref(),
                "crlf-result.txt",
                &"\n".repeat(4 * 1024 * 1024 + 1),
                "overwrite",
                Some(expected(b"old\r\n")),
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_FILE_TOO_LARGE".into())
        );
    }

    /// cancellation 在 acquire 前或同 target lock wait 时均返回 CANCELLED，且 canonical target 不变。
    #[tokio::test]
    async fn cancellation_before_acquire_and_while_waiting_lock_preserves_existing_target() {
        let _test_guard = test_guard();
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("held.txt");
        fs::write(&target, b"old\n").unwrap();
        let pre_cancel = CancellationToken::new();
        pre_cancel.cancel();
        assert_eq!(
            write(
                supervisor.as_ref(),
                "held.txt",
                "new",
                "overwrite",
                Some(expected(b"old\n")),
                pre_cancel,
            )
            .await,
            Err("CANCELLED".into())
        );

        let holder = existing_lock(supervisor.as_ref(), &workspace, "held.txt", b"old\n").await;
        let cancel = CancellationToken::new();
        let waiting = write(
            supervisor.as_ref(),
            "held.txt",
            "new",
            "overwrite",
            Some(expected(b"old\n")),
            cancel.clone(),
        );
        tokio::pin!(waiting);
        tokio::task::yield_now().await;
        cancel.cancel();
        assert_eq!(waiting.await, Err("CANCELLED".into()));
        drop(holder);
        assert_eq!(fs::read(target).unwrap(), b"old\n");
    }

    /// pre-read 分块取消与 lock 成功后 replace 前取消都保留 OLD；两个窗口都不触碰 canonical target。
    #[tokio::test]
    async fn cancellation_during_preread_and_after_existing_lock_preserves_old_bytes() {
        let _test_guard = test_guard();
        let (_directory, supervisor, _workspace, root) = fixture();
        let preread_target = root.join("preread.txt");
        let preread_bytes = vec![b'x'; 128 * 1024];
        fs::write(&preread_target, &preread_bytes).unwrap();
        let preread_cancel = CancellationToken::new();
        let preread_token = preread_cancel.clone();
        set_snapshot_chunk_hook_for_test(Arc::new(move || preread_token.cancel()));
        assert_eq!(
            write(
                supervisor.as_ref(),
                "preread.txt",
                "new",
                "overwrite",
                Some(expected(&preread_bytes)),
                preread_cancel,
            )
            .await,
            Err("CANCELLED".into())
        );
        assert_eq!(fs::read(&preread_target).unwrap(), preread_bytes);

        let lock_target = root.join("before-replace.txt");
        fs::write(&lock_target, b"old\n").unwrap();
        let lock_cancel = CancellationToken::new();
        let lock_token = lock_cancel.clone();
        set_after_existing_lock_hook_for_test(Arc::new(move || lock_token.cancel()));
        assert_eq!(
            write(
                supervisor.as_ref(),
                "before-replace.txt",
                "new",
                "overwrite",
                Some(expected(b"old\n")),
                lock_cancel,
            )
            .await,
            Err("CANCELLED".into())
        );
        assert_eq!(fs::read(lock_target).unwrap(), b"old\n");
    }

    /// pre-read 已验证 expected 版本后，同 target 外部编辑在锁等待期发生时由 P2C-003 lock 内重验拒绝。
    #[tokio::test]
    async fn external_edit_after_preread_while_waiting_lock_is_not_overwritten() {
        let _test_guard = test_guard();
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("race.txt");
        fs::write(&target, b"old\n").unwrap();
        let holder = existing_lock(supervisor.as_ref(), &workspace, "race.txt", b"old\n").await;
        let (ready_tx, ready_rx) = mpsc::channel();
        set_snapshot_ready_hook_for_test(Arc::new(move || ready_tx.send(()).unwrap()));
        let task_supervisor = Arc::clone(&supervisor);
        let task = tokio::spawn(async move {
            write(
                task_supervisor.as_ref(),
                "race.txt",
                "candidate",
                "overwrite",
                Some(expected(b"old\n")),
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

    /// snapshot 自行 resolver 前把已解析的 Workspace 子目录换成真实 junction；不得读取或覆盖外部文件。
    #[cfg(windows)]
    #[tokio::test]
    async fn snapshot_reresolve_rejects_junction_retarget_before_open() {
        let _test_guard = test_guard();
        let (directory, supervisor, _workspace, root) = fixture();
        let inside = root.join("swap");
        let outside = directory.path().join("outside");
        fs::create_dir(&inside).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(inside.join("source.txt"), b"inside\n").unwrap();
        fs::write(outside.join("source.txt"), b"outside\n").unwrap();
        let hook_inside = inside.clone();
        let hook_outside = outside.clone();
        set_snapshot_before_resolve_hook_for_test(Arc::new(move || {
            fs::remove_dir_all(&hook_inside).unwrap();
            let status = Command::new("cmd.exe")
                .args(["/c", "mklink", "/J"])
                .arg(&hook_inside)
                .arg(&hook_outside)
                .status()
                .unwrap();
            assert!(status.success());
        }));
        assert_eq!(
            write(
                supervisor.as_ref(),
                "swap/source.txt",
                "candidate",
                "overwrite",
                Some(expected(b"inside\n")),
                CancellationToken::new(),
            )
            .await,
            Err("WORKSPACE_CHANGED".into())
        );
        assert_eq!(fs::read(outside.join("source.txt")).unwrap(), b"outside\n");
    }

    /// Unix symlink 重定向与 Windows junction 走同一 snapshot re-resolve boundary，决不读取外部内容。
    #[cfg(unix)]
    #[tokio::test]
    async fn snapshot_reresolve_rejects_symlink_retarget_before_open() {
        use std::os::unix::fs::symlink;

        let _test_guard = test_guard();
        let (directory, supervisor, _workspace, root) = fixture();
        let inside = root.join("swap");
        let outside = directory.path().join("outside");
        fs::create_dir(&inside).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(inside.join("source.txt"), b"inside\n").unwrap();
        fs::write(outside.join("source.txt"), b"outside\n").unwrap();
        let hook_inside = inside.clone();
        let hook_outside = outside.clone();
        set_snapshot_before_resolve_hook_for_test(Arc::new(move || {
            fs::remove_dir_all(&hook_inside).unwrap();
            symlink(&hook_outside, &hook_inside).unwrap();
        }));
        assert_eq!(
            write(
                supervisor.as_ref(),
                "swap/source.txt",
                "candidate",
                "overwrite",
                Some(expected(b"inside\n")),
                CancellationToken::new(),
            )
            .await,
            Err("WORKSPACE_CHANGED".into())
        );
        assert_eq!(fs::read(outside.join("source.txt")).unwrap(), b"outside\n");
    }

    /// snapshot 读取中 target metadata/identity 变化必须在 replace 前以 version conflict 停止，外部 bytes 保留。
    #[tokio::test]
    async fn snapshot_identity_change_during_read_fails_closed_without_write() {
        let _test_guard = test_guard();
        let (_directory, supervisor, _workspace, root) = fixture();
        let target = root.join("identity.txt");
        let original = vec![b'a'; 128 * 1024];
        fs::write(&target, &original).unwrap();
        let external_target = target.clone();
        set_snapshot_chunk_hook_for_test(Arc::new(move || {
            fs::write(&external_target, b"external change\n").unwrap();
        }));
        assert_eq!(
            write(
                supervisor.as_ref(),
                "identity.txt",
                "candidate",
                "overwrite",
                Some(expected(&original)),
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_VERSION_CONFLICT".into())
        );
        assert_eq!(fs::read(target).unwrap(), b"external change\n");
    }

    /// shared resolver 继续拒绝缺失 parent 与所有常见 workspace escape，handler 不自动创建目录。
    #[tokio::test]
    async fn missing_parent_and_escape_paths_fail_closed() {
        let _test_guard = test_guard();
        let (_directory, supervisor, _workspace, root) = fixture();
        assert_eq!(
            write(
                supervisor.as_ref(),
                "missing/child.txt",
                "x",
                "fail",
                None,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_NOT_FOUND".into())
        );
        assert!(!root.join("missing").exists());
        for path in ["../outside.txt", "/outside.txt", "\\\\server\\share", "."] {
            assert_eq!(
                write(
                    supervisor.as_ref(),
                    path,
                    "x",
                    "fail",
                    None,
                    CancellationToken::new(),
                )
                .await,
                Err("SOURCE_PATH_OUTSIDE_WORKSPACE".into()),
                "{path}"
            );
        }
    }

    /// Windows 真实 junction 不能成为 whole-file write target；共享 resolver 必须在 handler 到达 commit 前拒绝。
    #[cfg(windows)]
    #[tokio::test]
    async fn rejects_actual_windows_junction_escape() {
        let _test_guard = test_guard();
        let (directory, supervisor, _workspace, root) = fixture();
        let outside = directory.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let status = Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(root.join("escape"))
            .arg(&outside)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            write(
                supervisor.as_ref(),
                "escape/new.txt",
                "x",
                "fail",
                None,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_PATH_OUTSIDE_WORKSPACE".into())
        );
        assert!(!outside.join("new.txt").exists());
    }

    /// Unix symlink 逃逸与 Windows junction 走同一 resolver 拒绝边界，保持跨平台 test contract。
    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_actual_unix_symlink_escape() {
        use std::os::unix::fs::symlink;

        let _test_guard = test_guard();
        let (directory, supervisor, _workspace, root) = fixture();
        let outside = directory.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        assert_eq!(
            write(
                supervisor.as_ref(),
                "escape/new.txt",
                "x",
                "fail",
                None,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_PATH_OUTSIDE_WORKSPACE".into())
        );
        assert!(!outside.join("new.txt").exists());
    }

    /// strict DTO 拒绝未知字段与非冻结 ifExists，Remote registry/list/dispatch 仍没有任何 Source Write tool。
    #[tokio::test]
    async fn dto_is_strict_and_remote_does_not_advertise_or_dispatch_source_write() {
        let _test_guard = test_guard();
        let (_directory, supervisor, _workspace, root) = fixture();
        for arguments in [
            json!({"workspaceId":"workspace", "relativePath":"never.txt", "content":"x", "ifExists":"fail"}),
            json!({"workspaceId":"workspace", "relative_path":"never.txt", "content":"x", "ifExists":"replace"}),
            json!({"workspaceId":"workspace", "relative_path":"never.txt", "content":"x", "ifExists":"fail", "root":"forbidden"}),
        ] {
            assert!(matches!(
                write_text_file(supervisor.as_ref(), arguments, CancellationToken::new()).await,
                Err(error) if error.starts_with("INVALID_PARAMS:")
            ));
        }
        for tool in super::super::source_write_domain::SourceWriteTool::ALL {
            assert!(
                !registry::list(false)
                    .iter()
                    .any(|tool_info| tool_info.name == tool.code())
            );
        }
        assert_eq!(
            crate::mcp::Broker::new(Arc::clone(&supervisor))
                .dispatch(
                    "source_write_text_file",
                    json!({"workspaceId":"workspace", "relative_path":"never.txt", "content":"x", "ifExists":"fail"}),
                    CancellationToken::new(),
                )
                .await,
            Err("UNKNOWN_TOOL".into())
        );
        assert!(fs::read_dir(root).unwrap().next().is_none());
    }

    /// commit unknown 的 Source Write taxonomy 不被 adapter 的普通 I/O 或 cancellation 分支重新映射。
    #[test]
    fn commit_state_unknown_projection_is_preserved() {
        let _test_guard = test_guard();
        assert_eq!(
            source_error(SourceWriteError::CommitStateUnknown),
            "SOURCE_COMMIT_STATE_UNKNOWN"
        );
    }

    /// 在同一 test binary 启动 child，让完整 handler 穿过 P2C-004 before/after replace checkpoint。
    fn run_crash_child(
        config_file: &Path,
        root: &Path,
        checkpoint: &str,
    ) -> std::process::ExitStatus {
        Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("mcp::source_write_file::tests::write_crash_child_process")
            .arg("--nocapture")
            .env("P2C004_CRASH_CHECKPOINT", checkpoint)
            .env("P2C007_CRASH_ROOT", root)
            .env("P2C007_CRASH_CONFIG", config_file)
            .status()
            .unwrap()
    }

    /// child 重建 Supervisor 并调用 production handler；checkpoint 直接 process::exit，绝不使用 drop 模拟 crash。
    #[test]
    fn write_crash_child_process() {
        let _test_guard = test_guard();
        let Ok(_root) = std::env::var("P2C007_CRASH_ROOT") else {
            return;
        };
        let config_file = PathBuf::from(std::env::var("P2C007_CRASH_CONFIG").unwrap());
        let state_directory = config_file.parent().unwrap().to_path_buf();
        let supervisor = SupervisorState::new(AppPaths {
            runtime_directory: state_directory.join("runtime"),
            config_file,
            log_directory: state_directory.join("logs"),
            app_log: state_directory.join("logs/app.log"),
            serena_log: state_directory.join("logs/serena.log"),
        })
        .unwrap();
        let expected_sha256 = expected(b"OLD complete file\n");
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(write_text_file(
                &supervisor,
                json!({
                    "workspaceId":"workspace",
                    "relative_path":"crash.txt",
                    "content":"NEW complete file\n",
                    "ifExists":"overwrite",
                    "expectedSha256":expected_sha256,
                }),
                CancellationToken::new(),
            ))
            .unwrap();
        panic!("crash checkpoint must terminate the child process");
    }

    /// handler 的 pre-replace crash 保留完整 OLD，post-replace crash 保留完整 NEW；不会留下半个 canonical target。
    #[tokio::test]
    async fn child_crashes_keep_existing_overwrite_atomic() {
        let _test_guard = test_guard();
        for (checkpoint, expected_bytes) in [
            ("before-replace", b"OLD complete file\n".as_slice()),
            ("after-replace", b"NEW complete file\n".as_slice()),
        ] {
            let (_directory, _supervisor, _workspace, root) = fixture();
            let config_file = root.parent().unwrap().join("config.json");
            fs::write(root.join("crash.txt"), b"OLD complete file\n").unwrap();
            let status = run_crash_child(&config_file, &root, checkpoint);
            assert_eq!(status.code(), Some(86), "{checkpoint}");
            assert_eq!(fs::read(root.join("crash.txt")).unwrap(), expected_bytes);
        }
    }
}
