//! P2C-006 的本地 create-only Source Write adapter；本模块不注册 Remote MCP Tool。
#![allow(
    dead_code,
    reason = "P2C-006 establishes the local create handler before P2C-012 advertises any Remote Source Write tool."
)]

use super::{
    registry,
    source_write_atomic_replace::{CreateNewFileError, create_new_file},
    source_write_commit::{TargetCommitError, lock_vacant_target},
    source_write_domain::{
        SourceWriteError, SourceWriteSuccess, SourceWriteTarget, validate_result_text_file_size,
        validate_whole_file_write_content,
    },
    source_write_support::{candidate_sha256, workspace_relative_path},
    source_write_text::{NewlineStyle, normalize_newlines},
};
use crate::serena::SupervisorState;
use serde::Deserialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// `source_create_text_file` 的严格本地输入；relative_path 按冻结 wire contract 保持 snake_case。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceCreateTextFileInput {
    workspace_id: String,
    #[serde(rename = "relative_path")]
    relative_path: String,
    content: String,
}

/// 解析、纯校验并在明确 Workspace Lease 内创建一个此前不存在的 UTF-8 文本文件。
pub(crate) async fn create_text_file(
    supervisor: &SupervisorState,
    arguments: Value,
    cancel: CancellationToken,
) -> Result<SourceWriteSuccess, String> {
    // Workspace taxonomy 先由既有 adapter 统一判定，不能让 serde 的字段错误改变 missing/null 分类。
    let workspace_id = registry::parse_workspace_id(&arguments)?;
    let input: SourceCreateTextFileInput =
        serde_json::from_value(arguments).map_err(|error| format!("INVALID_PARAMS: {error}"))?;
    debug_assert_eq!(input.workspace_id, workspace_id);
    create_text_file_input(supervisor, input, cancel).await
}

/// 输入已经完成 schema 分类后的 create handler；所有纯校验都发生在 Guard 或 filesystem 前。
async fn create_text_file_input(
    supervisor: &SupervisorState,
    input: SourceCreateTextFileInput,
    cancel: CancellationToken,
) -> Result<SourceWriteSuccess, String> {
    let target = SourceWriteTarget {
        workspace_id: input.workspace_id,
        relative_path: input.relative_path,
    };
    target.validate().map_err(source_error)?;
    validate_whole_file_write_content(&input.content).map_err(source_error)?;
    // 新文件固定使用 LF；normalize 不改变孤立 CR，也不补 final newline。
    let normalized = normalize_newlines(&input.content, NewlineStyle::Lf);
    validate_result_text_file_size(normalized.len()).map_err(source_error)?;
    let candidate = normalized.into_bytes();
    let after_sha256 = candidate_sha256(&candidate);

    if cancel.is_cancelled() {
        return Err("CANCELLED".into());
    }
    // 只从显式 workspaceId 取得同一 operation 临界区的 Lease 与 Guard，绝不读取 Desktop/legacy selection。
    let (lease, guard) = supervisor.resolve_workspace_write_guard(&target.workspace_id)?;
    let locked = tokio::select! {
        _ = cancel.cancelled() => return Err("CANCELLED".into()),
        result = lock_vacant_target(supervisor, lease.clone(), guard, &target.relative_path) => {
            result.map_err(target_commit_error)?
        }
    };
    // lock future 完成后先观察取消；这时尚未创建 temp 或 canonical target。
    if cancel.is_cancelled() {
        return Err("CANCELLED".into());
    }
    let path = workspace_relative_path(&lease.canonical_root, locked.canonical_target())?;

    match create_new_file(&locked, &candidate, &cancel) {
        Ok(()) => {}
        // publish 成功后的 cleanup 不会返回这里；其余 cancel 必须保持既有调用链的稳定投影。
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

/// 保持 P2C-001 Source Write string taxonomy，不把内部 enum 名泄露给 adapter 调用方。
fn source_error(error: SourceWriteError) -> String {
    error.code().into()
}

/// Target coordinator 已定义 WorkspaceChanged 与 Source Write 的唯一稳定投影。
fn target_commit_error(error: TargetCommitError) -> String {
    error.code().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::{product::AgentProductService, store::StateStore},
        config::{self, AppPaths, ManagerConfig, Workspace},
        mcp::{
            registry,
            source_write_atomic_replace::{
                CreateNewFileError, create_new_file,
                create_new_file_with_after_publish_hook_for_test,
                create_new_file_with_chunk_hook_for_test,
            },
            source_write_commit::lock_vacant_target,
        },
        workspace_registry::WORKSPACE_IN_USE,
    };
    use serde_json::json;
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::Arc,
    };

    /// child-process crash 的固定退出码，父进程据此区分预期强制退出与 harness failure。
    const CRASH_EXIT_CODE: i32 = 87;

    /// 构造注册且未选择的最小 Workspace，证明 handler 不依赖 Desktop 或 legacy active 状态。
    fn fixture() -> (
        tempfile::TempDir,
        Arc<SupervisorState>,
        Workspace,
        PathBuf,
        AppPaths,
    ) {
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
            Arc::new(SupervisorState::new(paths.clone()).unwrap()),
            workspace,
            root,
            paths,
        )
    }

    /// 以冻结 local DTO 调用 handler；测试不会通过 Remote registry 或 Broker dispatch 进入 write 路径。
    async fn create(
        supervisor: &SupervisorState,
        relative_path: &str,
        content: &str,
        cancel: CancellationToken,
    ) -> Result<SourceWriteSuccess, String> {
        create_text_file(
            supervisor,
            json!({
                "workspaceId":"workspace",
                "relative_path":relative_path,
                "content":content,
            }),
            cancel,
        )
        .await
    }

    /// 直接取得 P2C-003 vacant lock，供 primitive race、cancel 与 crash 测试复用。
    async fn vacant_lock(
        supervisor: &SupervisorState,
        workspace: &Workspace,
        relative_path: &str,
    ) -> super::super::source_write_commit::LockedTargetCommit {
        let (lease, guard) = supervisor
            .resolve_workspace_write_guard(&workspace.id)
            .unwrap();
        lock_vacant_target(supervisor, lease, guard, relative_path)
            .await
            .unwrap()
    }

    /// 新建文件统一 LF，空正文合法，结果 provenance 与 SHA 仅来自 captured Lease 和 normalized bytes。
    #[tokio::test]
    async fn creates_normalized_utf8_and_empty_files_with_stable_provenance() {
        let (_directory, supervisor, _workspace, root, _paths) = fixture();
        let created = create(
            supervisor.as_ref(),
            "src/new.txt",
            "one\r\ntwo\nthree\rfour",
            CancellationToken::new(),
        )
        .await;
        assert_eq!(created.unwrap_err(), "SOURCE_NOT_FOUND");
        fs::create_dir(root.join("src")).unwrap();

        let created = create(
            supervisor.as_ref(),
            "src/new.txt",
            "one\r\ntwo\nthree\rfour",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read(root.join("src/new.txt")).unwrap(),
            b"one\ntwo\nthree\rfour"
        );
        assert_eq!(created.path, "src/new.txt");
        assert_eq!(created.workspace_id, "workspace");
        assert_eq!(created.generation, 7);
        assert!(created.before_sha256.is_none());
        assert_eq!(
            created.after_sha256.as_str(),
            candidate_sha256(b"one\ntwo\nthree\rfour").as_str()
        );
        assert!(created.changed_range.is_none());
        assert!(created.changed_count.is_none());

        let empty = create(
            supervisor.as_ref(),
            "src/empty.txt",
            "",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(fs::read(root.join("src/empty.txt")).unwrap(), b"");
        assert_eq!(empty.after_sha256.as_str(), candidate_sha256(b"").as_str());
    }

    /// 已存在 target 在 lock 前拒绝，原始 bytes 不得被 temp 或 hard-link 路径改变。
    #[tokio::test]
    async fn existing_target_is_never_clobbered() {
        let (_directory, supervisor, _workspace, root, _paths) = fixture();
        let target = root.join("existing.txt");
        fs::write(&target, b"external bytes").unwrap();

        assert_eq!(
            create(
                supervisor.as_ref(),
                "existing.txt",
                "new",
                CancellationToken::new()
            )
            .await,
            Err("SOURCE_ALREADY_EXISTS".into())
        );
        assert_eq!(fs::read(target).unwrap(), b"external bytes");
    }

    /// NUL 与超限正文必须在 Workspace resolve 或 temp creation 前失败。
    #[tokio::test]
    async fn rejects_binary_and_oversized_input_before_filesystem_write() {
        let (_directory, supervisor, _workspace, root, _paths) = fixture();
        for content in [
            String::from("text\0binary"),
            "a".repeat(8 * 1024 * 1024 + 1),
        ] {
            assert!(matches!(
                create_text_file(
                    supervisor.as_ref(),
                    json!({"workspaceId":"workspace", "relative_path":"never.txt", "content":content}),
                    CancellationToken::new(),
                )
                .await,
                Err(error) if error == "SOURCE_BINARY_REJECTED" || error == "SOURCE_INPUT_LIMIT_EXCEEDED"
            ));
        }
        assert!(!root.join("never.txt").exists());
        assert!(fs::read_dir(&root).unwrap().next().is_none());
    }

    /// 缺失 parent 不得被 create helper 创建；路径 authority 仍由唯一 resolver/lock 提供。
    #[tokio::test]
    async fn missing_parent_and_workspace_escape_paths_fail_closed() {
        let (_directory, supervisor, _workspace, root, _paths) = fixture();
        assert_eq!(
            create(
                supervisor.as_ref(),
                "missing/child.txt",
                "x",
                CancellationToken::new()
            )
            .await,
            Err("SOURCE_NOT_FOUND".into())
        );
        assert!(!root.join("missing").exists());
        for path in ["../outside.txt", "/outside.txt", "\\\\server\\share", "."] {
            assert_eq!(
                create(supervisor.as_ref(), path, "x", CancellationToken::new()).await,
                Err("SOURCE_PATH_OUTSIDE_WORKSPACE".into()),
                "{path}"
            );
        }
    }

    /// Workspace 外链接绝不能成为 create target；Windows 使用真实 junction，Unix 使用真实 symlink。
    #[cfg(windows)]
    #[tokio::test]
    async fn rejects_actual_windows_junction_escape() {
        let (directory, supervisor, _workspace, root, _paths) = fixture();
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
            create(
                supervisor.as_ref(),
                "escape/new.txt",
                "x",
                CancellationToken::new()
            )
            .await,
            Err("SOURCE_PATH_OUTSIDE_WORKSPACE".into())
        );
        assert!(!outside.join("new.txt").exists());
    }

    /// Unix 的真实 symlink escape 与 Windows junction 同样必须由共享 resolver 拒绝。
    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_actual_unix_symlink_escape() {
        use std::os::unix::fs::symlink;

        let (directory, supervisor, _workspace, root, _paths) = fixture();
        let outside = directory.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        assert_eq!(
            create(
                supervisor.as_ref(),
                "escape/new.txt",
                "x",
                CancellationToken::new()
            )
            .await,
            Err("SOURCE_PATH_OUTSIDE_WORKSPACE".into())
        );
        assert!(!outside.join("new.txt").exists());
    }

    /// 同一 vacant key 的并发 create 至多一个成功，第二个在锁内观察已存在 target。
    #[tokio::test]
    async fn concurrent_same_target_create_has_one_winner() {
        let (_directory, supervisor, _workspace, root, _paths) = fixture();
        let (left, right) = tokio::join!(
            create(
                supervisor.as_ref(),
                "race.txt",
                "left",
                CancellationToken::new()
            ),
            create(
                supervisor.as_ref(),
                "race.txt",
                "right",
                CancellationToken::new()
            )
        );
        assert_eq!(
            [left.is_ok(), right.is_ok()]
                .into_iter()
                .filter(|ok| *ok)
                .count(),
            1
        );
        assert!(
            matches!(left, Err(ref error) if error == "SOURCE_ALREADY_EXISTS")
                || matches!(right, Err(ref error) if error == "SOURCE_ALREADY_EXISTS")
        );
        assert!(matches!(
            fs::read(root.join("race.txt")).unwrap().as_slice(),
            b"left" | b"right"
        ));
    }

    /// 外部进程/actor 在 vacant lock 后创建的 target 必须赢得所有权，hard-link 不得覆盖它。
    #[tokio::test]
    async fn external_create_between_lock_and_publish_is_not_overwritten() {
        let (_directory, supervisor, workspace, root, _paths) = fixture();
        let locked = vacant_lock(supervisor.as_ref(), &workspace, "external.txt").await;
        fs::write(root.join("external.txt"), b"external").unwrap();
        assert_eq!(
            create_new_file(&locked, b"candidate", &CancellationToken::new()),
            Err(CreateNewFileError::Source(SourceWriteError::AlreadyExists))
        );
        assert_eq!(fs::read(root.join("external.txt")).unwrap(), b"external");
    }

    /// 三类取消：预取消、锁等待取消与 temp 分块写入取消都不得创建 target。
    #[tokio::test]
    async fn cancellation_never_creates_a_partial_target() {
        let (_directory, supervisor, workspace, root, _paths) = fixture();
        let pre_cancel = CancellationToken::new();
        pre_cancel.cancel();
        assert_eq!(
            create(supervisor.as_ref(), "pre.txt", "x", pre_cancel).await,
            Err("CANCELLED".into())
        );

        let holder = vacant_lock(supervisor.as_ref(), &workspace, "wait.txt").await;
        let waiting_cancel = CancellationToken::new();
        let waiting = create(supervisor.as_ref(), "wait.txt", "x", waiting_cancel.clone());
        tokio::pin!(waiting);
        std::future::poll_fn(|context| {
            assert!(std::future::Future::poll(waiting.as_mut(), context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        waiting_cancel.cancel();
        assert_eq!(waiting.await, Err("CANCELLED".into()));
        drop(holder);
        assert!(!root.join("wait.txt").exists());

        let locked = vacant_lock(supervisor.as_ref(), &workspace, "write.txt").await;
        let chunk_cancel = CancellationToken::new();
        let token = chunk_cancel.clone();
        assert_eq!(
            create_new_file_with_chunk_hook_for_test(
                &locked,
                &vec![b'x'; 128 * 1024],
                &chunk_cancel,
                move |_| token.cancel(),
            ),
            Err(CreateNewFileError::Cancelled)
        );
        assert!(!root.join("write.txt").exists());
        assert!(
            fs::read_dir(&root)
                .unwrap()
                .filter_map(Result::ok)
                .all(|entry| !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".serena-source-write-"))
        );

        let locked = vacant_lock(supervisor.as_ref(), &workspace, "published.txt").await;
        let published_cancel = CancellationToken::new();
        let token = published_cancel.clone();
        assert_eq!(
            create_new_file_with_after_publish_hook_for_test(
                &locked,
                b"complete",
                &published_cancel,
                move || token.cancel(),
            ),
            Ok(())
        );
        assert_eq!(fs::read(root.join("published.txt")).unwrap(), b"complete");
    }

    /// six Source Write names 仍仅为 domain identity，不在 Remote registry advertise/list/dispatch 路径。
    #[test]
    fn source_write_names_remain_unadvertised_and_unroutable() {
        let advertised = registry::list(false);
        for tool in super::super::source_write_domain::SourceWriteTool::ALL {
            assert!(
                !advertised
                    .iter()
                    .any(|advertised| advertised.name == tool.code())
            );
        }
    }

    /// strict local DTO 拒绝额外字段，而 Remote 拒绝不影响同一 Local Host Rust handler。
    #[tokio::test]
    async fn local_host_create_handler_remains_callable_while_remote_registration_is_disabled() {
        let (_directory, supervisor, _workspace, root, _paths) = fixture();
        let local = create_text_file(
            supervisor.as_ref(),
            json!({"workspaceId":"workspace", "relative_path":"local.txt", "content":"local"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(local.path, "local.txt");
        assert_eq!(fs::read(root.join("local.txt")).unwrap(), b"local");
        assert_eq!(
            create_text_file(
                supervisor.as_ref(),
                json!({"relative_path":"never.txt", "content":"x"}),
                CancellationToken::new(),
            )
            .await,
            Err("WORKSPACE_CONTEXT_REQUIRED".into())
        );
        for input in [
            json!({"workspaceId":"workspace", "relativePath":"never.txt", "content":"x"}),
            json!({"workspaceId":"workspace", "relative_path":"never.txt", "content":"x", "root":"forbidden"}),
        ] {
            assert!(matches!(
                create_text_file(supervisor.as_ref(), input, CancellationToken::new()).await,
                Err(error) if error.starts_with("INVALID_PARAMS:")
            ));
        }
        assert_eq!(
            crate::mcp::Broker::new(Arc::clone(&supervisor))
                .dispatch(
                    "source_create_text_file",
                    json!({"workspaceId":"workspace", "relative_path":"never.txt", "content":"x"}),
                    CancellationToken::new(),
                )
                .await,
            Err("UNKNOWN_TOOL".into())
        );
        assert!(!root.join("never.txt").exists());
    }

    /// Guard 在 commit lock 持有期间继续阻止 Remove，helper 返回并 drop 后生命周期按原机制释放。
    #[tokio::test]
    async fn commit_lock_keeps_workspace_remove_blocked_until_drop() {
        let (directory, supervisor, workspace, root, _paths) = fixture();
        let product = AgentProductService::new(
            StateStore::open(directory.path().join("state"))
                .await
                .unwrap(),
        );
        let locked = vacant_lock(supervisor.as_ref(), &workspace, "held.txt").await;
        assert_eq!(
            supervisor
                .remove_workspace_coordinated(&product, &workspace.id)
                .await,
            Err(WORKSPACE_IN_USE.into())
        );
        create_new_file(&locked, b"complete", &CancellationToken::new()).unwrap();
        assert_eq!(fs::read(root.join("held.txt")).unwrap(), b"complete");
        drop(locked);
        assert_eq!(
            supervisor
                .remove_workspace_coordinated(&product, &workspace.id)
                .await
                .unwrap(),
            workspace
        );
    }

    /// 在同一 test binary 启动仅执行 create crash child 的进程，真实重开配置与 Workspace。
    fn run_crash_child(
        paths: &AppPaths,
        root: &Path,
        checkpoint: &str,
    ) -> std::process::ExitStatus {
        Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("mcp::source_write_create::tests::create_crash_child_process")
            .arg("--nocapture")
            .env("P2C006_CRASH_CHECKPOINT", checkpoint)
            .env("P2C006_CRASH_ROOT", root)
            .env("P2C006_CRASH_CONFIG", &paths.config_file)
            .status()
            .unwrap()
    }

    /// child 直接调用 production create primitive，在 full temp sync 前后两个 checkpoint 真实终止。
    #[test]
    fn create_crash_child_process() {
        let Ok(checkpoint) = std::env::var("P2C006_CRASH_CHECKPOINT") else {
            return;
        };
        let root = PathBuf::from(std::env::var("P2C006_CRASH_ROOT").unwrap());
        let config_file = PathBuf::from(std::env::var("P2C006_CRASH_CONFIG").unwrap());
        let state_directory = config_file.parent().unwrap().to_path_buf();
        let supervisor = SupervisorState::new(AppPaths {
            runtime_directory: state_directory.join("runtime"),
            config_file,
            log_directory: state_directory.join("logs"),
            app_log: state_directory.join("logs/app.log"),
            serena_log: state_directory.join("logs/serena.log"),
        })
        .unwrap();
        let workspace = Workspace {
            id: "workspace".into(),
            name: "Workspace".into(),
            root,
            generation: 7,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let locked = runtime.block_on(vacant_lock(&supervisor, &workspace, "crash.txt"));
        assert!(matches!(
            checkpoint.as_str(),
            "before-publish" | "after-publish"
        ));
        let _ = create_new_file(&locked, b"NEW complete file", &CancellationToken::new());
        panic!("crash checkpoint must terminate the child process");
    }

    /// pre-publish crash 绝不创建 target；post-publish crash 的 canonical target 必须完整 NEW，temp alias 可遗留。
    #[tokio::test]
    async fn child_crashes_preserve_create_atomicity() {
        for (checkpoint, target_exists) in [("before-publish", false), ("after-publish", true)] {
            let (_directory, _supervisor, _workspace, root, paths) = fixture();
            let status = run_crash_child(&paths, &root, checkpoint);
            assert_eq!(status.code(), Some(CRASH_EXIT_CODE), "{checkpoint}");
            let target = root.join("crash.txt");
            assert_eq!(target.exists(), target_exists, "{checkpoint}");
            if target_exists {
                assert_eq!(fs::read(target).unwrap(), b"NEW complete file");
            }
        }
    }
}
