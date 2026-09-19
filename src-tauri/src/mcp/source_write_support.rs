//! P2C-006/007 共用的内部 Source Write 支持函数；不持有 Workspace authority 或写入目标文件。
#![allow(
    dead_code,
    reason = "P2C-007 introduces local whole-file overwrite support before later Source Write handlers reuse the bounded snapshot helper."
)]

use super::{
    source_write_commit::{handle_snapshot, path_snapshot, same_file_snapshot},
    source_write_domain::{
        ExpectedSha256, SourceWriteError, validate_target_text_bytes,
        validate_target_text_file_size,
    },
};
use crate::{
    config::same_workspace_root_identity, workspace_path::WorkspacePathResolver,
    workspace_resolver::WorkspaceLease,
};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::{
    fs::{self, File},
    io::{ErrorKind, Read},
    path::Path,
};
use tokio_util::sync::CancellationToken;

/// 仅测试在 snapshot 每个读取 chunk 后注入取消，不进入 production 构建。
#[cfg(test)]
type SnapshotChunkHook = Arc<dyn Fn() + Send + Sync>;
/// 仅测试在完整 snapshot 验证后通知 handler 即将等待 commit lock，不进入 production 构建。
#[cfg(test)]
type SnapshotReadyHook = Arc<dyn Fn() + Send + Sync>;
/// 仅测试在 snapshot 自身 resolver 前重定向路径，覆盖 handler 初次 target-state resolve 后的 authority race。
#[cfg(test)]
type SnapshotBeforeResolveHook = Arc<dyn Fn() + Send + Sync>;
#[cfg(test)]
struct SnapshotTestHook<Hook> {
    target: std::path::PathBuf,
    action: Hook,
}
#[cfg(test)]
static SNAPSHOT_CHUNK_HOOK: OnceLock<Mutex<Option<SnapshotTestHook<SnapshotChunkHook>>>> =
    OnceLock::new();
#[cfg(test)]
static SNAPSHOT_READY_HOOK: OnceLock<Mutex<Option<SnapshotTestHook<SnapshotReadyHook>>>> =
    OnceLock::new();
#[cfg(test)]
static SNAPSHOT_BEFORE_RESOLVE_HOOK: OnceLock<
    Mutex<Option<SnapshotTestHook<SnapshotBeforeResolveHook>>>,
> = OnceLock::new();
/// 三类 snapshot hook 都是 test binary 进程级 seam，安装期间必须由同一 RAII guard 独占。
#[cfg(test)]
static SNAPSHOT_HOOK_TEST_SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

/// 独占一次性 snapshot hook，并在测试提前返回或 panic 时清除未消费的 hook。
#[cfg(test)]
pub(crate) struct SnapshotHookTestGuard {
    _serial: MutexGuard<'static, ()>,
}

/// 取得全局 snapshot hook 的测试所有权；仅 hook 相关用例串行，普通用例仍可并行。
#[cfg(test)]
pub(crate) fn snapshot_hook_test_guard() -> SnapshotHookTestGuard {
    SnapshotHookTestGuard {
        _serial: SNAPSHOT_HOOK_TEST_SERIAL
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    }
}

#[cfg(test)]
impl Drop for SnapshotHookTestGuard {
    /// 不让失败或提前返回的测试把未消费 hook 泄漏给后续测试。
    fn drop(&mut self) {
        *SNAPSHOT_CHUNK_HOOK
            .get_or_init(|| Mutex::new(None))
            .lock()
            .expect("snapshot chunk hook mutex poisoned") = None;
        *SNAPSHOT_READY_HOOK
            .get_or_init(|| Mutex::new(None))
            .lock()
            .expect("snapshot ready hook mutex poisoned") = None;
        *SNAPSHOT_BEFORE_RESOLVE_HOOK
            .get_or_init(|| Mutex::new(None))
            .lock()
            .expect("snapshot before-resolve hook mutex poisoned") = None;
    }
}

/// 设置一次性 snapshot chunk hook，供 P2C-007 cancellation regression 精确取消 pre-read。
#[cfg(test)]
pub(crate) fn set_snapshot_chunk_hook_for_test(target: &Path, hook: SnapshotChunkHook) {
    *SNAPSHOT_CHUNK_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some(SnapshotTestHook {
        target: fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf()),
        action: hook,
    });
}

/// 设置一次性 snapshot ready hook，供 P2C-007 精确制造 pre-read 后的外部编辑。
#[cfg(test)]
pub(crate) fn set_snapshot_ready_hook_for_test(target: &Path, hook: SnapshotReadyHook) {
    *SNAPSHOT_READY_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some(SnapshotTestHook {
        target: fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf()),
        action: hook,
    });
}

/// 设置一次性 pre-resolve hook，精确覆盖旧绝对路径 pre-read 的 junction/symlink race。
#[cfg(test)]
pub(crate) fn set_snapshot_before_resolve_hook_for_test(
    target: &Path,
    hook: SnapshotBeforeResolveHook,
) {
    *SNAPSHOT_BEFORE_RESOLVE_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some(SnapshotTestHook {
        target: fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf()),
        action: hook,
    });
}

/// 取走并调用一次 chunk hook，避免并行后续读取或后续测试重复触发。
#[cfg(test)]
fn run_snapshot_chunk_hook_for_test(target: &Path) {
    let hook = {
        let mut slot = SNAPSHOT_CHUNK_HOOK
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap();
        slot.as_ref()
            .is_some_and(|hook| hook.target == target)
            .then(|| slot.take().expect("matching snapshot chunk hook"))
    };
    if let Some(hook) = hook {
        (hook.action)();
    }
}

/// 取走并调用一次 ready hook，保证外部编辑发生在已验证 snapshot 与 commit lock 之间。
#[cfg(test)]
fn run_snapshot_ready_hook_for_test(target: &Path) {
    let hook = {
        let mut slot = SNAPSHOT_READY_HOOK
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap();
        slot.as_ref()
            .is_some_and(|hook| hook.target == target)
            .then(|| slot.take().expect("matching snapshot ready hook"))
    };
    if let Some(hook) = hook {
        (hook.action)();
    }
}

/// 在 snapshot 自己重新解析 Lease-relative path 前执行一次测试重定向，production 不包含此路径。
#[cfg(test)]
fn run_snapshot_before_resolve_hook_for_test(target: &Path) {
    let hook = {
        let mut slot = SNAPSHOT_BEFORE_RESOLVE_HOOK
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap();
        slot.as_ref()
            .is_some_and(|hook| hook.target == target)
            .then(|| slot.take().expect("matching snapshot before-resolve hook"))
    };
    if let Some(hook) = hook {
        (hook.action)();
    }
}

/// 从 raw candidate bytes 计算冻结的小写 SHA-256 version token。
pub(crate) fn candidate_sha256(candidate: &[u8]) -> ExpectedSha256 {
    let digest = Sha256::digest(candidate);
    ExpectedSha256::parse(
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )
    .expect("SHA-256 digest always has the frozen lowercase 64-hex shape")
}

/// 从 captured Lease root 与 P2C-003 canonical target 派生稳定 slash 相对路径。
pub(crate) fn workspace_relative_path(root: &Path, target: &Path) -> Result<String, String> {
    target
        .strip_prefix(root)
        .map_err(|_| SourceWriteError::PathOutsideWorkspace.code().to_owned())?
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .ok_or_else(|| SourceWriteError::PathOutsideWorkspace.code().to_owned())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|components| components.join("/"))
}

/// 已读取并通过 caller expected version 验证的 existing UTF-8 文本快照。
pub(crate) struct ExistingTextSnapshot {
    pub(crate) text: String,
    pub(crate) sha256: ExpectedSha256,
}

/// snapshot 读取的内部结果，保留取消与冻结 Source Write taxonomy 的边界。
pub(crate) enum ExistingTextSnapshotError {
    Source(SourceWriteError),
    WorkspaceChanged,
    Cancelled,
}

/// 从 captured Lease 与 relative_path 在实际读取边界重新解析、闭合 authority 后有界读取文本。
pub(crate) async fn read_existing_text_snapshot(
    lease: WorkspaceLease,
    relative_path: String,
    expected_sha256: ExpectedSha256,
    cancel: CancellationToken,
) -> Result<ExistingTextSnapshot, ExistingTextSnapshotError> {
    if cancel.is_cancelled() {
        return Err(ExistingTextSnapshotError::Cancelled);
    }
    let worker_cancel = cancel.clone();
    let task = tokio::task::spawn_blocking(move || {
        read_existing_text_snapshot_blocking(
            &lease,
            &relative_path,
            &expected_sha256,
            &worker_cancel,
        )
    });
    tokio::select! {
        result = task => result.map_err(|_| ExistingTextSnapshotError::Source(SourceWriteError::IoError))?,
        _ = cancel.cancelled() => Err(ExistingTextSnapshotError::Cancelled),
    }
}

/// 在 blocking file read 中以 Lease-rooted resolver 闭合路径/handle identity，并逐块检查取消与 8 MiB 上界。
fn read_existing_text_snapshot_blocking(
    lease: &WorkspaceLease,
    relative_path: &str,
    expected_sha256: &ExpectedSha256,
    cancel: &CancellationToken,
) -> Result<ExistingTextSnapshot, ExistingTextSnapshotError> {
    #[cfg(test)]
    run_snapshot_before_resolve_hook_for_test(&lease.canonical_root.join(relative_path));
    let canonical_target = resolve_snapshot_target(lease, relative_path)?;
    let metadata = fs::symlink_metadata(&canonical_target).map_err(map_snapshot_io_error)?;
    if !is_plain_target(&metadata) {
        return Err(ExistingTextSnapshotError::WorkspaceChanged);
    }
    let metadata_len = usize::try_from(metadata.len())
        .map_err(|_| ExistingTextSnapshotError::Source(SourceWriteError::FileTooLarge))?;
    validate_target_text_file_size(metadata_len).map_err(ExistingTextSnapshotError::Source)?;

    let mut file = File::open(&canonical_target).map_err(map_snapshot_io_error)?;
    let opened_snapshot = handle_snapshot(&file).map_err(map_snapshot_identity_error)?;
    let path_before = verified_path_snapshot(lease, relative_path, &canonical_target)?;
    if !same_file_snapshot(&opened_snapshot, &path_before) {
        return Err(ExistingTextSnapshotError::Source(
            SourceWriteError::VersionConflict,
        ));
    }
    let mut bytes = Vec::with_capacity(metadata_len);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if cancel.is_cancelled() {
            return Err(ExistingTextSnapshotError::Cancelled);
        }
        let read = file.read(&mut buffer).map_err(map_snapshot_io_error)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        validate_target_text_file_size(bytes.len()).map_err(ExistingTextSnapshotError::Source)?;
        #[cfg(test)]
        run_snapshot_chunk_hook_for_test(&canonical_target);
        if cancel.is_cancelled() {
            return Err(ExistingTextSnapshotError::Cancelled);
        }
    }

    let text = validate_target_text_bytes(&bytes)
        .map_err(ExistingTextSnapshotError::Source)?
        .to_owned();
    let sha256 = candidate_sha256(&bytes);
    let handle_after = handle_snapshot(&file).map_err(map_snapshot_identity_error)?;
    let path_after = verified_path_snapshot(lease, relative_path, &canonical_target)?;
    if bytes.len() as u64 != metadata.len()
        || !same_file_snapshot(&opened_snapshot, &handle_after)
        || !same_file_snapshot(&opened_snapshot, &path_after)
    {
        return Err(ExistingTextSnapshotError::Source(
            SourceWriteError::VersionConflict,
        ));
    }
    if &sha256 != expected_sha256 {
        return Err(ExistingTextSnapshotError::Source(
            SourceWriteError::VersionConflict,
        ));
    }
    #[cfg(test)]
    run_snapshot_ready_hook_for_test(&canonical_target);
    Ok(ExistingTextSnapshot { text, sha256 })
}

/// 仅以共享 WorkspacePathResolver 从 captured Lease 重新解析，不信任任何调用方给出的绝对路径。
fn resolve_snapshot_target(
    lease: &WorkspaceLease,
    relative_path: &str,
) -> Result<std::path::PathBuf, ExistingTextSnapshotError> {
    WorkspacePathResolver::new(lease)
        .resolve(relative_path)
        .map_err(|_| ExistingTextSnapshotError::WorkspaceChanged)
}

/// 同时重跑 resolver、比较 canonical target 并拒绝 symlink/reparse，再取得 P2C-003 共用 path identity snapshot。
fn verified_path_snapshot(
    lease: &WorkspaceLease,
    relative_path: &str,
    opened_target: &Path,
) -> Result<super::source_write_commit::FileSnapshot, ExistingTextSnapshotError> {
    let resolved = resolve_snapshot_target(lease, relative_path)?;
    if !same_workspace_root_identity(&resolved, opened_target) {
        return Err(ExistingTextSnapshotError::WorkspaceChanged);
    }
    let metadata = fs::symlink_metadata(&resolved).map_err(map_snapshot_io_error)?;
    if !is_plain_target(&metadata) {
        return Err(ExistingTextSnapshotError::WorkspaceChanged);
    }
    path_snapshot(&resolved).map_err(map_snapshot_identity_error)
}

/// snapshot read boundary 使用 symlink_metadata，拒绝 Unix symlink 与 Windows reparse point。
fn is_plain_target(metadata: &fs::Metadata) -> bool {
    metadata.is_file() && !metadata.file_type().is_symlink() && !is_reparse_point(metadata)
}

/// Windows reparse 属性不能由普通 is_file 替代，必须在 read boundary 明确拒绝。
#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & 0x400 != 0
}

/// Unix 的 symlink 已由 file_type 表示，额外 reparse 属性不适用。
#[cfg(not(windows))]
fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

/// P2C-003 identity helper 失败时，先复核 resolver boundary；逃逸/reparse 绝不降级成普通 version conflict。
fn map_snapshot_identity_error(
    _error: super::source_write_commit::TargetCommitError,
) -> ExistingTextSnapshotError {
    ExistingTextSnapshotError::WorkspaceChanged
}

/// existing target 在 pre-read 与 lock 之间消失属于版本漂移；其余 I/O 维持既有 taxonomy。
fn map_snapshot_io_error(error: std::io::Error) -> ExistingTextSnapshotError {
    let source = if error.kind() == ErrorKind::NotFound {
        SourceWriteError::VersionConflict
    } else {
        SourceWriteError::IoError
    };
    ExistingTextSnapshotError::Source(source)
}
