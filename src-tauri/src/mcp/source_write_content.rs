//! P2C-011 仅实现本地 `source_replace_content`；Remote registry 与 Broker dispatch 保持关闭。
#![allow(
    dead_code,
    reason = "P2C-011 implements the local handler before a later task explicitly advertises Remote Source Write."
)]

use super::{
    registry,
    source_write_atomic_replace::replace_existing_file,
    source_write_commit::{TargetCommitError, lock_existing_target},
    source_write_domain::{
        SourceWriteError, SourceWriteSuccess, VersionedSourceWriteTarget,
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

/// 内容替换只接受冻结的两个 literal replacement mode。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ReplaceContentMode {
    First,
    All,
}

/// `source_replace_content` 的严格本地输入；relative_path 保持冻结 snake_case wire 名。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceReplaceContentInput {
    #[serde(flatten)]
    target: VersionedSourceWriteTarget,
    old_content: String,
    new_content: String,
    mode: ReplaceContentMode,
    expected_matches: Option<u32>,
    max_replacements: Option<u32>,
}

/// 在显式 Workspace Lease 中按冻结 literal 规则替换既有 UTF-8 文本内容。
pub(crate) async fn replace_content(
    supervisor: &SupervisorState,
    arguments: Value,
    cancel: CancellationToken,
) -> Result<SourceWriteSuccess, String> {
    // Workspace taxonomy 先由既有 adapter 统一判定，serde 不得改变 missing/null 的稳定分类。
    let workspace_id = registry::parse_workspace_id(&arguments)?;
    let input: SourceReplaceContentInput =
        serde_json::from_value(arguments).map_err(|error| format!("INVALID_PARAMS: {error}"))?;
    debug_assert_eq!(input.target.workspace_id, workspace_id);
    replace_content_input(supervisor, input, cancel).await
}

/// schema 解析后的 handler；所有纯输入校验在 Guard 与 filesystem access 前完成。
async fn replace_content_input(
    supervisor: &SupervisorState,
    input: SourceReplaceContentInput,
    cancel: CancellationToken,
) -> Result<SourceWriteSuccess, String> {
    input.target.validate().map_err(source_error)?;
    validate_replace_content_input(&input).map_err(source_error)?;
    if cancel.is_cancelled() {
        return Err("CANCELLED".into());
    }

    // 只从 caller 的显式 workspaceId 取得 captured Lease 与 RAII Guard，绝不回退到 UI 或 session state。
    let (lease, guard) = supervisor.resolve_workspace_write_guard(&input.target.workspace_id)?;
    WorkspacePathResolver::new(&lease)
        .resolve(&input.target.relative_path)
        .map_err(|_| source_error(SourceWriteError::PathOutsideWorkspace))?;

    // pre-read 只产生与 expected version 匹配的 bounded snapshot；literal 匹配和 newline policy 均只从此 snapshot 取得。
    let snapshot = read_existing_text_snapshot(
        lease.clone(),
        input.target.relative_path.clone(),
        input.target.expected_sha256.clone(),
        cancel.clone(),
    )
    .await
    .map_err(snapshot_error)?;
    let actual_matches = count_literal_matches(&snapshot.text, &input.old_content);

    // 冻结优先级要求 no-match 先于数量约束；此时仍未触碰 commit primitive。
    if actual_matches == 0 {
        return Err(source_error(SourceWriteError::ContentNotFound));
    }
    validate_match_constraints(&input, actual_matches).map_err(source_error)?;
    let newline_style = detect_newline_style(&snapshot.text);
    let replacement = normalize_newlines(&input.new_content, newline_style);
    // 该 no-op 依赖 snapshot newline policy，必须在无匹配和数量约束之后、commit 前拒绝。
    if replacement == input.old_content {
        return Err(source_error(SourceWriteError::InvalidArgument));
    }
    let replacement_count = match input.mode {
        ReplaceContentMode::First => 1,
        ReplaceContentMode::All => actual_matches,
    };
    // `replacen` 是 literal 且从左到右 non-overlapping；只构造已被匹配约束准许的 candidate。
    let candidate = snapshot
        .text
        .replacen(&input.old_content, &replacement, replacement_count);
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
        changed_range: None,
        changed_count: Some(replacement_count as u32),
    };
    success.validate().map_err(source_error)?;
    Ok(success)
}

/// 纯参数校验固定先于 Workspace、path 和文件访问。
fn validate_replace_content_input(
    input: &SourceReplaceContentInput,
) -> Result<(), SourceWriteError> {
    if input.old_content.is_empty() || input.old_content == input.new_content {
        return Err(SourceWriteError::InvalidArgument);
    }
    validate_inline_mutation_content(&input.old_content)?;
    validate_inline_mutation_content(&input.new_content)?;
    if input.expected_matches == Some(0) {
        return Err(SourceWriteError::InvalidArgument);
    }
    match input.mode {
        ReplaceContentMode::First if input.max_replacements.is_some() => {
            Err(SourceWriteError::InvalidArgument)
        }
        ReplaceContentMode::All => {
            let max_replacements = input
                .max_replacements
                .ok_or(SourceWriteError::InvalidArgument)?;
            if max_replacements == 0
                || input
                    .expected_matches
                    .is_some_and(|expected_matches| expected_matches > max_replacements)
            {
                return Err(SourceWriteError::InvalidArgument);
            }
            Ok(())
        }
        ReplaceContentMode::First => Ok(()),
    }
}

/// 从左至右消耗每个 literal match，固定不把重叠 substring 再次计数。
fn count_literal_matches(text: &str, old_content: &str) -> usize {
    text.match_indices(old_content).count()
}

/// snapshot 读取后按冻结优先级验证期望匹配数和 all 上限。
fn validate_match_constraints(
    input: &SourceReplaceContentInput,
    actual_matches: usize,
) -> Result<(), SourceWriteError> {
    if input
        .expected_matches
        .is_some_and(|expected_matches| actual_matches != expected_matches as usize)
    {
        return Err(SourceWriteError::ContentAmbiguous);
    }
    if matches!(input.mode, ReplaceContentMode::All)
        && actual_matches > input.max_replacements.expect("validated max replacement") as usize
    {
        return Err(SourceWriteError::ContentAmbiguous);
    }
    Ok(())
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
        sync::{Arc, Mutex, MutexGuard, OnceLock, mpsc},
        time::Duration,
    };

    static TEST_SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

    /// 获取 P2C-011 handler 测试的唯一 hook ownership，释放后才允许下一条测试安装 hook。
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
        old_content: &str,
        new_content: &str,
        mode: &str,
        expected_matches: Option<u32>,
        max_replacements: Option<u32>,
        cancel: CancellationToken,
    ) -> Result<SourceWriteSuccess, String> {
        replace_content(
            supervisor,
            json!({
                "workspaceId":"workspace",
                "relative_path":relative_path,
                "expectedSha256":expected_sha256,
                "oldContent":old_content,
                "newContent":new_content,
                "mode":mode,
                "expectedMatches":expected_matches,
                "maxReplacements":max_replacements,
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

    /// `aaa` 中的 `aa` 只能按 non-overlapping 规则计为一次，first 成功只改第一处。
    #[tokio::test]
    async fn replaces_first_non_overlapping_literal_match_with_provenance() {
        let (_directory, supervisor, _workspace, root) = fixture();
        let existing = b"aaa";
        fs::write(root.join("first.txt"), existing).unwrap();
        let result = replace(
            supervisor.as_ref(),
            "first.txt",
            expected(existing),
            "aa",
            "b",
            "first",
            Some(1),
            None,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(fs::read(root.join("first.txt")).unwrap(), b"ba");
        assert_eq!(result.changed_range, None);
        assert_eq!(result.changed_count, Some(1));
        assert_eq!(result.before_sha256.unwrap().as_str(), expected(existing));
        assert_eq!(result.after_sha256.as_str(), expected(b"ba"));
    }

    /// first 的多匹配允许只替换第一处；提供的 expectedMatches 必须比较修改前全文计数。
    #[tokio::test]
    async fn first_replaces_only_once_and_rejects_expected_match_mismatch() {
        let (_directory, supervisor, _workspace, root) = fixture();
        let existing = b"one one one";
        fs::write(root.join("first.txt"), existing).unwrap();
        let success = replace(
            supervisor.as_ref(),
            "first.txt",
            expected(existing),
            "one",
            "two",
            "first",
            None,
            None,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(success.changed_count, Some(1));
        assert_eq!(fs::read(root.join("first.txt")).unwrap(), b"two one one");
        fs::write(root.join("first.txt"), existing).unwrap();
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "first.txt",
                expected(existing),
                "one",
                "two",
                "first",
                Some(1),
                None,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_CONTENT_AMBIGUOUS".into())
        );
        assert_eq!(fs::read(root.join("first.txt")).unwrap(), existing);
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "first.txt",
                expected(existing),
                "absent",
                "two",
                "first",
                Some(1),
                None,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_CONTENT_NOT_FOUND".into())
        );
        assert_eq!(fs::read(root.join("first.txt")).unwrap(), existing);
    }

    /// all 只在匹配数与上限均满足时提交全部 non-overlapping replacement。
    #[tokio::test]
    async fn replaces_all_and_rejects_not_found_expected_mismatch_and_cap() {
        let (_directory, supervisor, _workspace, root) = fixture();
        let existing = b"one one one";
        fs::write(root.join("all.txt"), existing).unwrap();
        let result = replace(
            supervisor.as_ref(),
            "all.txt",
            expected(existing),
            "one",
            "two",
            "all",
            Some(3),
            Some(3),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result.changed_count, Some(3));
        assert_eq!(fs::read(root.join("all.txt")).unwrap(), b"two two two");
        fs::write(root.join("all.txt"), existing).unwrap();
        for (old_content, expected_matches, max_replacements, error) in [
            ("absent", Some(1), Some(1), "SOURCE_CONTENT_NOT_FOUND"),
            ("one", Some(2), Some(3), "SOURCE_CONTENT_AMBIGUOUS"),
            ("one", None, Some(2), "SOURCE_CONTENT_AMBIGUOUS"),
        ] {
            assert_eq!(
                replace(
                    supervisor.as_ref(),
                    "all.txt",
                    expected(existing),
                    old_content,
                    "two",
                    "all",
                    expected_matches,
                    max_replacements,
                    CancellationToken::new(),
                )
                .await,
                Err(error.into())
            );
            assert_eq!(fs::read(root.join("all.txt")).unwrap(), existing);
        }
    }

    /// 所有冻结纯参数错误都先于未知 Workspace 与文件访问稳定失败。
    #[tokio::test]
    async fn rejects_invalid_parameters_before_workspace_or_file_access() {
        let (_directory, supervisor, _workspace, _root) = fixture();
        for arguments in [
            json!({"workspaceId":"missing","relative_path":"never.txt","expectedSha256":"a".repeat(64),"oldContent":"","newContent":"x","mode":"first"}),
            json!({"workspaceId":"missing","relative_path":"never.txt","expectedSha256":"a".repeat(64),"oldContent":"x","newContent":"x","mode":"first"}),
            json!({"workspaceId":"missing","relative_path":"never.txt","expectedSha256":"a".repeat(64),"oldContent":"x","newContent":"y","mode":"first","expectedMatches":0}),
            json!({"workspaceId":"missing","relative_path":"never.txt","expectedSha256":"a".repeat(64),"oldContent":"x","newContent":"y","mode":"all"}),
            json!({"workspaceId":"missing","relative_path":"never.txt","expectedSha256":"a".repeat(64),"oldContent":"x","newContent":"y","mode":"all","maxReplacements":0}),
            json!({"workspaceId":"missing","relative_path":"never.txt","expectedSha256":"a".repeat(64),"oldContent":"x","newContent":"y","mode":"first","maxReplacements":1}),
            json!({"workspaceId":"missing","relative_path":"never.txt","expectedSha256":"a".repeat(64),"oldContent":"x","newContent":"y","mode":"all","expectedMatches":2,"maxReplacements":1}),
        ] {
            assert_eq!(
                replace_content(supervisor.as_ref(), arguments, CancellationToken::new()).await,
                Err("SOURCE_INVALID_ARGUMENT".into())
            );
        }
    }

    /// 特殊字符按 literal 处理，空 replacement 删除匹配内容而非引入 regex 语义。
    #[tokio::test]
    async fn replaces_literal_special_characters_and_allows_empty_replacement() {
        let (_directory, supervisor, _workspace, root) = fixture();
        let existing = b"a.*?[]b a.*?[]b";
        fs::write(root.join("literal.txt"), existing).unwrap();
        let result = replace(
            supervisor.as_ref(),
            "literal.txt",
            expected(existing),
            ".*?[]",
            "",
            "all",
            Some(2),
            Some(2),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result.changed_count, Some(2));
        assert_eq!(fs::read(root.join("literal.txt")).unwrap(), b"ab ab");
    }

    /// LF、CRLF 与 mixed target 均只规范化 replacement，未匹配 raw bytes 保持不变。
    #[tokio::test]
    async fn normalizes_replacement_without_rewriting_untouched_bytes() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for (path, existing, expected_bytes) in [
            (
                "lf.txt",
                b"left\nTARGET\nright\r".as_slice(),
                b"left\nnew\nvalue\nright\r".as_slice(),
            ),
            (
                "crlf.txt",
                b"left\r\nTARGET\r\nright\n".as_slice(),
                b"left\r\nnew\r\nvalue\r\nright\n".as_slice(),
            ),
            (
                "mixed.txt",
                b"left\r\nTARGET\nright\r\n".as_slice(),
                b"left\r\nnew\r\nvalue\nright\r\n".as_slice(),
            ),
        ] {
            fs::write(root.join(path), existing).unwrap();
            replace(
                supervisor.as_ref(),
                path,
                expected(existing),
                "TARGET",
                "new\nvalue",
                "first",
                Some(1),
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(fs::read(root.join(path)).unwrap(), expected_bytes);
        }
    }

    /// replacement 经目标 newline policy 规范化后等于 oldContent 时不得执行无意义 replace。
    #[tokio::test]
    async fn rejects_normalization_induced_noop_without_commit() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for (path, existing, old_content, new_content, mode, expected_matches, max_replacements) in [
            (
                "lf.txt",
                b"prefix a\nb suffix".as_slice(),
                "a\nb",
                "a\r\nb",
                "first",
                Some(1),
                None,
            ),
            (
                "crlf.txt",
                b"prefix a\r\nb suffix".as_slice(),
                "a\r\nb",
                "a\nb",
                "all",
                Some(1),
                Some(1),
            ),
        ] {
            fs::write(root.join(path), existing).unwrap();
            assert_eq!(
                replace(
                    supervisor.as_ref(),
                    path,
                    expected(existing),
                    old_content,
                    new_content,
                    mode,
                    expected_matches,
                    max_replacements,
                    CancellationToken::new(),
                )
                .await,
                Err("SOURCE_INVALID_ARGUMENT".into())
            );
            assert_eq!(fs::read(root.join(path)).unwrap(), existing);
        }
    }

    /// NUL、inline input、target 与 result hard limits，以及 stale SHA 都不得进入 commit。
    #[tokio::test]
    async fn rejects_nul_all_size_limits_and_stale_sha_without_commit() {
        let (_directory, supervisor, _workspace, root) = fixture();
        let existing = b"target";
        fs::write(root.join("limits.txt"), existing).unwrap();
        let input_limit = "x".repeat(1024 * 1024 + 1);
        for (old_content, new_content, expected_error) in [
            ("target", "\0", "SOURCE_BINARY_REJECTED"),
            (input_limit.as_str(), "new", "SOURCE_INPUT_LIMIT_EXCEEDED"),
            (
                "target",
                input_limit.as_str(),
                "SOURCE_INPUT_LIMIT_EXCEEDED",
            ),
        ] {
            assert_eq!(
                replace(
                    supervisor.as_ref(),
                    "limits.txt",
                    expected(existing),
                    old_content,
                    new_content,
                    "first",
                    None,
                    None,
                    CancellationToken::new(),
                )
                .await,
                Err(expected_error.into())
            );
            assert_eq!(fs::read(root.join("limits.txt")).unwrap(), existing);
        }
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "limits.txt",
                "b".repeat(64),
                "target",
                "new",
                "first",
                None,
                None,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_VERSION_CONFLICT".into())
        );
        let oversized = vec![b'x'; 8 * 1024 * 1024 + 1];
        fs::write(root.join("oversized.txt"), &oversized).unwrap();
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "oversized.txt",
                expected(&oversized),
                "x",
                "y",
                "first",
                None,
                None,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_FILE_TOO_LARGE".into())
        );
        let result_target = vec![b'x'; 8 * 1024 * 1024];
        fs::write(root.join("result.txt"), &result_target).unwrap();
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "result.txt",
                expected(&result_target),
                "x",
                "xy",
                "first",
                None,
                None,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_FILE_TOO_LARGE".into())
        );
        assert_eq!(fs::read(root.join("result.txt")).unwrap(), result_target);
    }

    /// external edit、Workspace generation 漂移与 cancellation 均在 commit 前 fail closed。
    #[tokio::test]
    async fn rejects_external_edit_workspace_drift_and_cancellation_without_commit() {
        let _guard = test_guard();
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("race.txt");
        let existing = b"one\ntwo\n";
        fs::write(&target, existing).unwrap();
        let holder = existing_lock(supervisor.as_ref(), &workspace, "race.txt", existing).await;
        let (ready_tx, ready_rx) = mpsc::channel();
        set_snapshot_ready_hook_for_test(Arc::new(move || ready_tx.send(()).unwrap()));
        let task_supervisor = Arc::clone(&supervisor);
        let task = tokio::spawn(async move {
            replace(
                task_supervisor.as_ref(),
                "race.txt",
                expected(existing),
                "one",
                "candidate",
                "first",
                None,
                None,
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
        assert_eq!(fs::read(&target).unwrap(), b"external\n");

        let drift = root.join("drift.txt");
        fs::write(&drift, existing).unwrap();
        let changed_supervisor = Arc::clone(&supervisor);
        let mut changed_workspace = workspace;
        changed_workspace.generation += 1;
        set_snapshot_ready_hook_for_test(Arc::new(move || {
            changed_supervisor
                .replace_workspaces(vec![changed_workspace.clone()])
                .unwrap();
        }));
        assert_eq!(
            replace(
                supervisor.as_ref(),
                "drift.txt",
                expected(existing),
                "one",
                "candidate",
                "first",
                None,
                None,
                CancellationToken::new(),
            )
            .await,
            Err("WORKSPACE_CHANGED".into())
        );
        assert_eq!(fs::read(&drift).unwrap(), existing);

        let current_workspace = Workspace {
            id: "workspace".into(),
            name: "Workspace".into(),
            root: root.clone(),
            generation: 8,
        };
        let holder = existing_lock(
            supervisor.as_ref(),
            &current_workspace,
            "race.txt",
            b"external\n",
        )
        .await;
        let (ready_tx, ready_rx) = mpsc::channel();
        set_snapshot_ready_hook_for_test(Arc::new(move || ready_tx.send(()).unwrap()));
        let cancel = CancellationToken::new();
        let waiting_cancel = cancel.clone();
        let waiting_supervisor = Arc::clone(&supervisor);
        let waiting = tokio::spawn(async move {
            replace(
                waiting_supervisor.as_ref(),
                "race.txt",
                expected(b"external\n"),
                "external",
                "candidate",
                "first",
                None,
                None,
                waiting_cancel,
            )
            .await
        });
        tokio::task::spawn_blocking(move || ready_rx.recv_timeout(Duration::from_secs(2)))
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while supervisor
                .target_commit_coordinator()
                .permit_count_for_test()
                != 2
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        cancel.cancel();
        assert_eq!(waiting.await.unwrap(), Err("CANCELLED".into()));
        drop(holder);
        assert_eq!(fs::read(&target).unwrap(), b"external\n");
    }

    /// Windows 真实 junction 不能成为 content replacement target，shared resolver 必须在 snapshot 前拒绝。
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
                "outside",
                "inside",
                "first",
                None,
                None,
                CancellationToken::new(),
            )
            .await,
            Err("SOURCE_PATH_OUTSIDE_WORKSPACE".into())
        );
        assert_eq!(fs::read(outside.join("target.txt")).unwrap(), b"outside\n");
    }

    /// strict DTO 与 Remote registry/dispatch 均不能把本地 handler 暴露成 MCP Tool。
    #[tokio::test]
    async fn local_dto_is_strict_and_remote_does_not_advertise_or_dispatch_replace_content() {
        let (_directory, supervisor, _workspace, root) = fixture();
        for arguments in [
            json!({"workspaceId":"workspace","relativePath":"never.txt","expectedSha256":"a".repeat(64),"oldContent":"x","newContent":"y","mode":"first"}),
            json!({"workspaceId":"workspace","relative_path":"never.txt","expectedSha256":"a".repeat(64),"oldContent":"x","newContent":"y","mode":"first","unknown":true}),
        ] {
            assert!(
                replace_content(supervisor.as_ref(), arguments, CancellationToken::new())
                    .await
                    .is_err()
            );
        }
        assert!(
            !registry::list(false)
                .iter()
                .any(|tool| tool.name == "source_replace_content")
        );
        assert_eq!(
            Broker::new(Arc::clone(&supervisor))
                .dispatch(
                    "source_replace_content",
                    json!({}),
                    CancellationToken::new(),
                )
                .await,
            Err("UNKNOWN_TOOL".into())
        );
        assert_eq!(fs::read(root.join("never.txt")).ok(), None);
    }
}
