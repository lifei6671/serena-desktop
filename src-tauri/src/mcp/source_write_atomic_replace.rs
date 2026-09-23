//! Source Write 既有文件的 crash-safe 原子替换；本模块不提供任何 public Tool handler。
#![allow(
    dead_code,
    reason = "P2C-004 establishes the future existing-file primitive before P2C-007 handlers consume it."
)]

#[cfg(windows)]
use super::source_write_domain::{ExpectedSha256, TARGET_TEXT_FILE_MAX};
use super::{
    source_write_commit::LockedTargetCommit,
    source_write_domain::{SourceWriteError, validate_result_text_file_size},
};
#[cfg(windows)]
use sha2::Digest as _;
#[cfg(windows)]
use std::io::Read;
use std::io::{self, ErrorKind, Write};
use std::{
    fs::{self, File, Metadata},
    path::Path,
};
use tokio_util::sync::CancellationToken;

/// create primitive 的内部结果；仅区分既有 Source Write 错误与调用链已有的 CANCELLED。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreateNewFileError {
    Source(SourceWriteError),
    Cancelled,
}

impl From<SourceWriteError> for CreateNewFileError {
    /// 不建立第二套字符串 taxonomy，Source Write 错误保持原样。
    fn from(error: SourceWriteError) -> Self {
        Self::Source(error)
    }
}

/// 将已锁定的 existing target 完整替换为 candidate bytes，并保持调用方持有的 commit lock。
pub(crate) fn replace_existing_file(
    locked: &LockedTargetCommit,
    candidate: &[u8],
) -> Result<(), SourceWriteError> {
    // 必须先拒绝超限结果，避免在创建 temp 或读取 target metadata 前触碰文件系统。
    validate_result_text_file_size(candidate.len())?;

    // 目标路径只来自 P2C-003 锁内 revalidation，绝不接受 caller path。
    let target = locked.canonical_target();
    let parent = target.parent().ok_or(SourceWriteError::IoError)?;
    let parent_metadata = fs::metadata(parent).map_err(map_io_error)?;
    if !parent_metadata.is_dir() {
        return Err(SourceWriteError::IoError);
    }
    let target_metadata = fs::metadata(target).map_err(map_io_error)?;
    if !target_metadata.is_file() {
        return Err(SourceWriteError::IoError);
    }

    // tempfile_in 强制在 target 同目录创建唯一文件，替换不会跨 filesystem。
    let mut temporary = tempfile::Builder::new()
        .prefix(".serena-source-write-")
        .tempfile_in(parent)
        .map_err(map_io_error)?;
    write_and_sync_candidate(temporary.as_file_mut(), candidate).map_err(map_io_error)?;
    preserve_basic_metadata(temporary.as_file(), &target_metadata).map_err(map_io_error)?;
    // 关闭 temp handle 后仍保留 TempPath 的 RAII cleanup；Windows ReplaceFileW 不能依赖该 handle 继续打开。
    let temporary = temporary.into_temp_path();

    // 该测试 checkpoint 位于完整 temp durable 后、任何 target replacement 前。
    #[cfg(test)]
    crash_at_checkpoint(CrashCheckpoint::BeforeReplace);

    commit_existing_replacement(locked, temporary.as_ref(), candidate)?;

    // 该测试 checkpoint 位于 replacement 成功后、parent sync 与正常返回前。
    #[cfg(test)]
    crash_at_checkpoint(CrashCheckpoint::AfterReplace);

    // rename 已经提交 NEW；目录同步仅尽力提高断电后的持久性，绝不能把已提交结果改投影为 Err。
    sync_parent_directory(parent);
    Ok(())
}

/// 将已锁定的 vacant target 以完整、已同步的同目录 temp 原子发布；绝不接受裸 caller path。
pub(crate) fn create_new_file(
    locked: &LockedTargetCommit,
    candidate: &[u8],
    cancel: &CancellationToken,
) -> Result<(), CreateNewFileError> {
    create_new_file_inner(locked, candidate, cancel, |_| {}, || {})
}

/// create 的唯一 commit coordinator：先完成并同步 temp，再以 hard link 无覆盖地声明 target ownership。
fn create_new_file_inner<ChunkHook, PublishHook>(
    locked: &LockedTargetCommit,
    candidate: &[u8],
    cancel: &CancellationToken,
    mut on_chunk: ChunkHook,
    on_published: PublishHook,
) -> Result<(), CreateNewFileError>
where
    ChunkHook: FnMut(usize),
    PublishHook: FnOnce(),
{
    // 该 helper 只能消费 P2C-003 已确认 vacant 的 target，existing replacement 一律走自己的原语。
    if locked.before_sha256().is_some() {
        return Err(SourceWriteError::IoError.into());
    }
    validate_result_text_file_size(candidate.len()).map_err(CreateNewFileError::from)?;
    if cancel.is_cancelled() {
        return Err(CreateNewFileError::Cancelled);
    }

    // target 与 parent 只来自 LockedTargetCommit；不自动创建目录，也不重新解析 caller path。
    let target = locked.canonical_target();
    let parent = target.parent().ok_or(SourceWriteError::IoError)?;
    let parent_metadata = fs::metadata(parent).map_err(map_io_error)?;
    if !parent_metadata.is_dir() {
        return Err(SourceWriteError::IoError.into());
    }
    let mut temporary = tempfile::Builder::new()
        .prefix(".serena-source-write-")
        .tempfile_in(parent)
        .map_err(map_io_error)?;

    // target 从未被打开写入；取消或写入失败时 NamedTempFile RAII 仅清理临时别名。
    write_and_sync_new_candidate(temporary.as_file_mut(), candidate, cancel, &mut on_chunk)
        .map_err(|error| {
            if cancel.is_cancelled() {
                CreateNewFileError::Cancelled
            } else {
                map_io_error(error).into()
            }
        })?;
    let temporary = temporary.into_temp_path();

    // temp 已完整且 durable，但尚未发布 target；crash 只能留下 orphan temp。
    #[cfg(test)]
    crash_at_create_checkpoint(CreateCrashCheckpoint::BeforePublish);

    if cancel.is_cancelled() {
        return Err(CreateNewFileError::Cancelled);
    }
    // hard_link 是 no-clobber 的原子 ownership 决定：target 已存在时决不覆盖或跟随它。
    let temporary_path: &Path = temporary.as_ref();
    fs::hard_link(temporary_path, target).map_err(map_create_publish_error)?;
    on_published();

    // 此后 canonical target 已完整 NEW；cleanup 与目录 sync 均不得把成功投影成失败。
    #[cfg(test)]
    crash_at_create_checkpoint(CreateCrashCheckpoint::AfterPublish);
    drop(temporary);
    sync_parent_directory(parent);
    Ok(())
}

/// 分块写入 create candidate，在每个块前后观察 cancellation，随后 flush 与 sync temp。
fn write_and_sync_new_candidate<Hook>(
    file: &mut File,
    candidate: &[u8],
    cancel: &CancellationToken,
    on_chunk: &mut Hook,
) -> io::Result<()>
where
    Hook: FnMut(usize),
{
    const WRITE_CHUNK_BYTES: usize = 64 * 1024;

    for chunk in candidate.chunks(WRITE_CHUNK_BYTES) {
        if cancel.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "source write cancelled",
            ));
        }
        file.write_all(chunk)?;
        on_chunk(chunk.len());
        if cancel.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "source write cancelled",
            ));
        }
    }
    file.flush()?;
    file.sync_all()
}

/// 将 no-clobber publish 的已存在冲突保留为 Source Write 稳定分类，其余 failure 均 fail closed。
fn map_create_publish_error(error: io::Error) -> SourceWriteError {
    if error.kind() == ErrorKind::AlreadyExists {
        SourceWriteError::AlreadyExists
    } else {
        SourceWriteError::IoError
    }
}

/// 测试专用 chunk checkpoint；production API 不暴露写入节流或 hook。
#[cfg(test)]
pub(crate) fn create_new_file_with_chunk_hook_for_test<Hook>(
    locked: &LockedTargetCommit,
    candidate: &[u8],
    cancel: &CancellationToken,
    on_chunk: Hook,
) -> Result<(), CreateNewFileError>
where
    Hook: FnMut(usize),
{
    create_new_file_inner(locked, candidate, cancel, on_chunk, || {})
}

/// 测试专用 post-publish checkpoint；取消发生在 target ownership 提交后仍必须报告成功。
#[cfg(test)]
pub(crate) fn create_new_file_with_after_publish_hook_for_test<Hook>(
    locked: &LockedTargetCommit,
    candidate: &[u8],
    cancel: &CancellationToken,
    on_published: Hook,
) -> Result<(), CreateNewFileError>
where
    Hook: FnOnce(),
{
    create_new_file_inner(locked, candidate, cancel, |_| {}, on_published)
}

/// 将 candidate 一次性写入 temp，并在替换前将数据同步到持久介质边界。
fn write_and_sync_candidate(file: &mut File, candidate: &[u8]) -> io::Result<()> {
    file.write_all(candidate)?;
    file.flush()?;
    file.sync_all()
}

/// Unix 在 rename 前把 target mode bits 应用于 temp，避免默认 0600 泄漏到结果文件。
#[cfg(unix)]
fn preserve_basic_metadata(file: &File, target_metadata: &Metadata) -> io::Result<()> {
    file.set_permissions(target_metadata.permissions())?;
    // chmod 也是 metadata 变更，必须在 replacement 前同步。
    file.sync_all()
}

/// Windows ReplaceFileW 负责沿用原 target 的 metadata、ACL 与 attributes 语义，不能手工迁移。
#[cfg(windows)]
fn preserve_basic_metadata(_file: &File, _target_metadata: &Metadata) -> io::Result<()> {
    Ok(())
}

/// Unix/macOS 的同目录 rename 是原子 existing-file replacement。
#[cfg(not(windows))]
fn commit_existing_replacement(
    locked: &LockedTargetCommit,
    temporary: &Path,
    _candidate: &[u8],
) -> Result<(), SourceWriteError> {
    fs::rename(temporary, locked.canonical_target()).map_err(map_io_error)
}

/// Windows 原生替换失败的安全分类；仅文档明确的 1176/1177 需要后续只读对账。
#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowsReplaceFailure {
    OrdinaryIo,
    AmbiguousCommitState,
}

/// Windows 只使用 ReplaceFileW 覆盖已有 target，不采用 delete+rename 或 copy-overwrite。
#[cfg(windows)]
fn commit_existing_replacement(
    locked: &LockedTargetCommit,
    temporary: &Path,
    candidate: &[u8],
) -> Result<(), SourceWriteError> {
    match atomic_replace(temporary, locked.canonical_target()) {
        Ok(()) => Ok(()),
        Err(WindowsReplaceFailure::OrdinaryIo) => Err(SourceWriteError::IoError),
        // 1176/1177 不承诺 target 仍在原路径；在锁内仅对 canonical target 做有界只读对账。
        Err(WindowsReplaceFailure::AmbiguousCommitState) => {
            reconcile_ambiguous_replace(locked, candidate)
        }
    }
}

/// 调用 ReplaceFileW 并在失败时立即读取 Win32 error，避免后续 FFI 覆盖 LastError。
#[cfg(windows)]
fn atomic_replace(temporary: &Path, target: &Path) -> Result<(), WindowsReplaceFailure> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{Foundation::GetLastError, Storage::FileSystem::ReplaceFileW};

    /// 将 Windows 文件路径转换为带 NUL 结尾的 Win32 UTF-16 参数。
    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let target = wide_path(target);
    let temporary = wide_path(temporary);
    // SAFETY: 两个缓冲区在调用期间保持存活且均以 NUL 结尾；backup、exclude、preserved 按 API 允许为空。
    let replaced = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            temporary.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            ptr::null(),
        )
    };
    if replaced == 0 {
        // 必须紧随失败调用读取 LastError；之后不得以普通 IO error 淹没 1176/1177 的不确定状态。
        return Err(classify_windows_replace_failure(unsafe { GetLastError() }));
    }
    Ok(())
}

/// 按 ReplaceFileW 文档区分可证明未提交的普通失败与路径可能已变化的失败。
#[cfg(windows)]
fn classify_windows_replace_failure(error_code: u32) -> WindowsReplaceFailure {
    use windows_sys::Win32::Foundation::{
        ERROR_UNABLE_TO_MOVE_REPLACEMENT, ERROR_UNABLE_TO_MOVE_REPLACEMENT_2,
    };

    if error_code == ERROR_UNABLE_TO_MOVE_REPLACEMENT
        || error_code == ERROR_UNABLE_TO_MOVE_REPLACEMENT_2
    {
        WindowsReplaceFailure::AmbiguousCommitState
    } else {
        WindowsReplaceFailure::OrdinaryIo
    }
}

/// 对账观察值故意只描述 canonical target；任何路径、链接或读取不确定性都不推断已提交。
#[cfg(windows)]
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReconciliationObservation {
    Current(ExpectedSha256),
    Missing,
    Uncertain,
}

/// 仅在文档化的 ambiguous ReplaceFileW failure 后，对 canonical target 决定安全投影。
#[cfg(windows)]
fn reconcile_ambiguous_replace(
    locked: &LockedTargetCommit,
    candidate: &[u8],
) -> Result<(), SourceWriteError> {
    let candidate_sha256 = raw_sha256(candidate);
    decide_reconciled_commit(
        observe_canonical_target(locked.canonical_target()),
        &candidate_sha256,
        locked.before_sha256(),
    )
}

/// 只有精确 NEW 与精确 OLD 可被证明；其余情形保持原生调用后的不确定状态。
#[cfg(windows)]
fn decide_reconciled_commit(
    observation: ReconciliationObservation,
    candidate_sha256: &ExpectedSha256,
    before_sha256: Option<&ExpectedSha256>,
) -> Result<(), SourceWriteError> {
    match observation {
        ReconciliationObservation::Current(actual) if &actual == candidate_sha256 => Ok(()),
        ReconciliationObservation::Current(actual) if Some(&actual) == before_sha256 => {
            Err(SourceWriteError::IoError)
        }
        ReconciliationObservation::Current(_)
        | ReconciliationObservation::Missing
        | ReconciliationObservation::Uncertain => Err(SourceWriteError::CommitStateUnknown),
    }
}

/// 在 8 MiB 硬界内读取 canonical target，并拒绝 link/reparse、非普通文件及任何 TOCTOU 不确定性。
#[cfg(windows)]
fn observe_canonical_target(target: &Path) -> ReconciliationObservation {
    let initial_metadata = match fs::symlink_metadata(target) {
        Ok(metadata) if is_plain_windows_file(&metadata) => metadata,
        Ok(_) => return ReconciliationObservation::Uncertain,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return ReconciliationObservation::Missing;
        }
        Err(_) => return ReconciliationObservation::Uncertain,
    };
    if initial_metadata.len() > TARGET_TEXT_FILE_MAX as u64 {
        return ReconciliationObservation::Uncertain;
    }

    let mut opened = match File::open(target) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return ReconciliationObservation::Missing;
        }
        Err(_) => return ReconciliationObservation::Uncertain,
    };
    let opened_metadata = match opened.metadata() {
        Ok(metadata) if metadata.is_file() && metadata.len() <= TARGET_TEXT_FILE_MAX as u64 => {
            metadata
        }
        _ => return ReconciliationObservation::Uncertain,
    };
    let actual_sha256 = match raw_sha256_from_file(&mut opened) {
        Ok(sha256) => sha256,
        Err(_) => return ReconciliationObservation::Uncertain,
    };
    let current_metadata = match fs::symlink_metadata(target) {
        Ok(metadata) if is_plain_windows_file(&metadata) => metadata,
        Ok(_) | Err(_) => return ReconciliationObservation::Uncertain,
    };
    if current_metadata.len() > TARGET_TEXT_FILE_MAX as u64 {
        return ReconciliationObservation::Uncertain;
    }
    let current = match File::open(target) {
        Ok(file) => file,
        Err(_) => return ReconciliationObservation::Uncertain,
    };
    if !same_windows_file_identity(&opened, &current)
        || opened_metadata.len() != current_metadata.len()
    {
        return ReconciliationObservation::Uncertain;
    }
    ReconciliationObservation::Current(actual_sha256)
}

/// Windows reparse point 或非普通文件不能成为 ambiguous-result reconciliation 的可信事实。
#[cfg(windows)]
fn is_plain_windows_file(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.is_file() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
}

/// 以固定缓冲区计算原始 SHA-256，并在读取中持续执行 8 MiB 上界。
#[cfg(windows)]
fn raw_sha256_from_file(file: &mut File) -> io::Result<ExpectedSha256> {
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_usize;
    while total < TARGET_TEXT_FILE_MAX {
        // 每次只请求剩余限额，避免 reconciliation 在外部文件增长时读取超过 8 MiB。
        let remaining = TARGET_TEXT_FILE_MAX - total;
        let bytes_to_read = remaining.min(buffer.len());
        let read = file.read(&mut buffer[..bytes_to_read])?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read)
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "source target size overflow"))?;
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    ExpectedSha256::parse(
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )
    .map_err(|_| io::Error::new(ErrorKind::InvalidData, "invalid SHA-256 digest"))
}

/// 对比 file handle identity，避免 canonical path 在读取期间被替换后误判 SHA。
#[cfg(windows)]
fn same_windows_file_identity(left: &File, right: &File) -> bool {
    windows_file_identity(left)
        .zip(windows_file_identity(right))
        .is_some_and(|(left, right)| left == right)
}

/// 从已打开 handle 获取 Windows file identity；读取失败即由调用方归为不确定。
#[cfg(windows)]
fn windows_file_identity(file: &File) -> Option<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
    };

    let mut information = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
    // SAFETY: file handle 有效，information 指向 API 要填充的初始化内存。
    let read =
        unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut information) };
    if read == 0 {
        return None;
    }
    Some((
        information.dwVolumeSerialNumber,
        ((information.nFileIndexHigh as u64) << 32) | information.nFileIndexLow as u64,
    ))
}

/// 计算已验证 candidate 的原始 SHA-256；调用方已在入口执行相同的 8 MiB 上限检查。
#[cfg(windows)]
fn raw_sha256(bytes: &[u8]) -> ExpectedSha256 {
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    ExpectedSha256::parse(
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )
    .expect("SHA-256 hex digest always satisfies ExpectedSha256")
}

/// Unix 在 rename 后尽力同步 parent directory entry；任何失败都不能改变已经提交的成功结果。
#[cfg(unix)]
fn sync_parent_directory(parent: &Path) {
    let _ = File::open(parent).and_then(|directory| directory.sync_all());
}

/// Windows 没有同等可靠的 std/Win32 目录 flush 路径，因此 replacement 后不追加失败步骤。
#[cfg(windows)]
fn sync_parent_directory(_parent: &Path) {}

/// 将所有 filesystem failure 保持投影为冻结的 SOURCE_IO_ERROR。
fn map_io_error(_error: io::Error) -> SourceWriteError {
    SourceWriteError::IoError
}

/// 仅测试二进制识别的 create crash 位置，production build 不包含任何注入路径。
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreateCrashCheckpoint {
    BeforePublish,
    AfterPublish,
}

/// 仅在 P2C-006 child-process test 指定的位置退出，复用 production create primitive 的真实序列。
#[cfg(test)]
fn crash_at_create_checkpoint(checkpoint: CreateCrashCheckpoint) {
    let expected = match checkpoint {
        CreateCrashCheckpoint::BeforePublish => "before-publish",
        CreateCrashCheckpoint::AfterPublish => "after-publish",
    };
    if std::env::var("P2C006_CRASH_CHECKPOINT").as_deref() == Ok(expected) {
        std::process::exit(87);
    }
}

/// 仅测试二进制识别的 crash 位置，production build 不包含任何注入路径。
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum CrashCheckpoint {
    BeforeReplace,
    AfterReplace,
}

/// 仅在 child-process test 指定的位置直接退出，借此验证生产替换序列的真实文件系统状态。
#[cfg(test)]
fn crash_at_checkpoint(checkpoint: CrashCheckpoint) {
    let expected = match checkpoint {
        CrashCheckpoint::BeforeReplace => "before-replace",
        CrashCheckpoint::AfterReplace => "after-replace",
    };
    if std::env::var("P2C004_CRASH_CHECKPOINT").as_deref() == Ok(expected) {
        std::process::exit(86);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::{product::AgentProductService, store::StateStore},
        config::{self, AppPaths, ManagerConfig, Workspace},
        mcp::source_write_commit::{LockedTargetCommit, lock_existing_target},
        serena::{SupervisorState, WorkspaceWriteGuard},
        workspace_registry::WORKSPACE_IN_USE,
        workspace_resolver::WorkspaceLease,
    };
    use sha2::{Digest, Sha256};
    use std::{
        path::{Path, PathBuf},
        process::Command,
        sync::Arc,
    };

    /// child-process crash 的固定退出码，父进程用它区分预期强制退出与 harness failure。
    const CRASH_EXIT_CODE: i32 = 86;

    /// 建立可被独立 child reopen 的最小 Workspace、Supervisor 与普通 existing target root。
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

    /// 计算 P2C-003 获取 existing-file commit lock 所需的 raw SHA-256 token。
    fn expected_sha256(bytes: &[u8]) -> super::super::source_write_domain::ExpectedSha256 {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hasher.finalize();
        super::super::source_write_domain::ExpectedSha256::parse(
            digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        )
        .unwrap()
    }

    /// 按 P2C-003 authority 流程取得 writer 专属 Lease 与 WorkspaceWriteGuard。
    fn writer(
        supervisor: &SupervisorState,
        workspace: &Workspace,
    ) -> (WorkspaceLease, WorkspaceWriteGuard) {
        supervisor
            .resolve_workspace_write_guard(&workspace.id)
            .unwrap()
    }

    /// 获取完整 locked target，保证所有 primitive test 均通过真实 P2C-003 前置锁。
    async fn lock_target(
        supervisor: &SupervisorState,
        workspace: &Workspace,
        relative_path: &str,
        old: &[u8],
    ) -> LockedTargetCommit {
        let (lease, guard) = writer(supervisor, workspace);
        lock_existing_target(
            supervisor,
            lease,
            guard,
            relative_path,
            expected_sha256(old),
        )
        .await
        .unwrap()
    }

    /// 返回本 primitive 创建的 temp 路径，隔离 fixture 中不存在其他同前缀文件。
    fn replace_temp_paths(parent: &Path) -> Vec<PathBuf> {
        fs::read_dir(parent)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".serena-source-write-"))
            })
            .collect()
    }

    /// 在同一 test binary 中启动仅执行 crash child case 的进程。
    fn run_crash_child(
        paths: &AppPaths,
        root: &Path,
        checkpoint: &str,
    ) -> std::process::ExitStatus {
        Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("mcp::source_write_atomic_replace::tests::crash_child_process")
            .arg("--nocapture")
            .env("P2C004_CRASH_CHECKPOINT", checkpoint)
            .env("P2C004_CRASH_ROOT", root)
            .env("P2C004_CRASH_CONFIG", &paths.config_file)
            .status()
            .unwrap()
    }

    /// child process 重建真实 Supervisor/LockedTargetCommit 后调用同一 production helper 并在 checkpoint 退出。
    #[test]
    fn crash_child_process() {
        let Ok(checkpoint) = std::env::var("P2C004_CRASH_CHECKPOINT") else {
            return;
        };
        let root = PathBuf::from(std::env::var("P2C004_CRASH_ROOT").unwrap());
        let config_file = PathBuf::from(std::env::var("P2C004_CRASH_CONFIG").unwrap());
        let state_directory = config_file.parent().unwrap().to_path_buf();
        let paths = AppPaths {
            runtime_directory: state_directory.join("runtime"),
            config_file,
            log_directory: state_directory.join("logs"),
            app_log: state_directory.join("logs/app.log"),
            serena_log: state_directory.join("logs/serena.log"),
        };
        let supervisor = SupervisorState::new(paths).unwrap();
        let workspace = Workspace {
            id: "workspace".into(),
            name: "Workspace".into(),
            root,
            generation: 7,
        };
        let old = b"OLD complete file";
        let (lease, guard) = writer(&supervisor, &workspace);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let locked = runtime
            .block_on(lock_existing_target(
                &supervisor,
                lease,
                guard,
                "crash.txt",
                expected_sha256(old),
            ))
            .unwrap();
        let candidate = match checkpoint.as_str() {
            "before-replace" | "after-replace" => b"NEW complete file".as_slice(),
            _ => panic!("unknown P2C-004 crash checkpoint"),
        };
        let _ = replace_existing_file(&locked, candidate);
        panic!("crash checkpoint must terminate the child process");
    }

    /// 正常替换必须完整写入 NEW，并且 helper 不会自行释放调用方的 LockedTargetCommit。
    #[tokio::test]
    async fn successful_atomic_replace_keeps_commit_lock_owned_by_caller() {
        let (_directory, supervisor, workspace, root, _paths) = fixture();
        let target = root.join("target.txt");
        fs::write(&target, b"OLD complete file").unwrap();
        let locked = lock_target(
            supervisor.as_ref(),
            &workspace,
            "target.txt",
            b"OLD complete file",
        )
        .await;

        replace_existing_file(&locked, b"NEW complete file").unwrap();

        assert_eq!(fs::read(target).unwrap(), b"NEW complete file");
        assert!(replace_temp_paths(&root).is_empty());
        drop(locked);
    }

    /// 8 MiB+1 在创建 temp 或读取 target metadata 前失败，原 target 与目录内容均不得改变。
    #[tokio::test]
    async fn oversized_candidate_fails_before_temp_or_target_mutation() {
        let (_directory, supervisor, workspace, root, _paths) = fixture();
        let target = root.join("too-large.txt");
        fs::write(&target, b"OLD complete file").unwrap();
        let locked = lock_target(
            supervisor.as_ref(),
            &workspace,
            "too-large.txt",
            b"OLD complete file",
        )
        .await;

        let candidate = vec![b'N'; 8 * 1024 * 1024 + 1];
        assert_eq!(
            replace_existing_file(&locked, &candidate),
            Err(SourceWriteError::FileTooLarge)
        );
        assert_eq!(fs::read(target).unwrap(), b"OLD complete file");
        assert!(replace_temp_paths(&root).is_empty());
    }

    /// Unix replacement 必须沿用已有 target 的 mode bits，而不是泄漏 tempfile 默认 0600。
    #[cfg(unix)]
    #[tokio::test]
    async fn unix_replace_preserves_target_mode_bits() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let (_directory, supervisor, workspace, root, _paths) = fixture();
        let target = root.join("mode.txt");
        fs::write(&target, b"OLD complete file").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o751)).unwrap();
        let expected_mode = fs::metadata(&target).unwrap().mode() & 0o7777;
        let locked = lock_target(
            supervisor.as_ref(),
            &workspace,
            "mode.txt",
            b"OLD complete file",
        )
        .await;

        replace_existing_file(&locked, b"NEW complete file").unwrap();

        assert_eq!(fs::metadata(target).unwrap().mode() & 0o7777, expected_mode);
    }

    /// Unix post-commit directory sync 的类型刻意不可失败，结构上保证 rename 后不会再向 caller 返回 Err。
    #[cfg(unix)]
    #[test]
    fn unix_post_commit_directory_sync_is_infallible() {
        fn assert_infallible(_: fn(&Path)) {}

        assert_infallible(sync_parent_directory);
    }

    /// Windows 必须通过真实 ReplaceFileW 替换已有 target，完整结果不得依赖 std::fs::rename 覆盖语义。
    #[cfg(windows)]
    #[tokio::test]
    async fn windows_replace_file_replaces_existing_target() {
        let (_directory, supervisor, workspace, root, _paths) = fixture();
        let target = root.join("windows-replace.txt");
        fs::write(&target, b"OLD complete file").unwrap();
        let locked = lock_target(
            supervisor.as_ref(),
            &workspace,
            "windows-replace.txt",
            b"OLD complete file",
        )
        .await;

        replace_existing_file(&locked, b"NEW complete file").unwrap();

        assert_eq!(fs::read(target).unwrap(), b"NEW complete file");
    }

    /// 仅 1176/1177 按 ReplaceFileW 文档进入受限对账；1175 与其他错误仍是普通安全 IO 失败。
    #[cfg(windows)]
    #[test]
    fn windows_replace_error_classifier_marks_only_documented_ambiguous_codes() {
        use windows_sys::Win32::Foundation::{
            ERROR_UNABLE_TO_MOVE_REPLACEMENT, ERROR_UNABLE_TO_MOVE_REPLACEMENT_2,
            ERROR_UNABLE_TO_REMOVE_REPLACED,
        };

        assert_eq!(
            classify_windows_replace_failure(ERROR_UNABLE_TO_MOVE_REPLACEMENT),
            WindowsReplaceFailure::AmbiguousCommitState
        );
        assert_eq!(
            classify_windows_replace_failure(ERROR_UNABLE_TO_MOVE_REPLACEMENT_2),
            WindowsReplaceFailure::AmbiguousCommitState
        );
        assert_eq!(
            classify_windows_replace_failure(ERROR_UNABLE_TO_REMOVE_REPLACED),
            WindowsReplaceFailure::OrdinaryIo
        );
        assert_eq!(
            classify_windows_replace_failure(5),
            WindowsReplaceFailure::OrdinaryIo
        );
    }

    /// ambiguous native failure 后，canonical target 的完整 candidate SHA 是已提交 NEW 的充分证据。
    #[cfg(windows)]
    #[test]
    fn windows_reconciliation_candidate_sha_is_success() {
        let old = expected_sha256(b"OLD complete file");
        let new = expected_sha256(b"NEW complete file");

        assert_eq!(
            decide_reconciled_commit(
                ReconciliationObservation::Current(new.clone()),
                &new,
                Some(&old),
            ),
            Ok(())
        );
    }

    /// ambiguous native failure 后，canonical target 的完整 before SHA 是可证明的 safe non-commit。
    #[cfg(windows)]
    #[test]
    fn windows_reconciliation_before_sha_is_safe_io_failure() {
        let old = expected_sha256(b"OLD complete file");
        let new = expected_sha256(b"NEW complete file");

        assert_eq!(
            decide_reconciled_commit(
                ReconciliationObservation::Current(old.clone()),
                &new,
                Some(&old),
            ),
            Err(SourceWriteError::IoError)
        );
    }

    /// target 缺失、不同正文以及 reparse/unreadable 等观察不确定时必须保留 UNKNOWN 而非伪装安全失败。
    #[cfg(windows)]
    #[test]
    fn windows_reconciliation_missing_other_and_reparse_are_unknown() {
        let old = expected_sha256(b"OLD complete file");
        let new = expected_sha256(b"NEW complete file");
        let other = expected_sha256(b"OTHER complete file");
        for observation in [
            ReconciliationObservation::Missing,
            ReconciliationObservation::Current(other),
            // Uncertain 是 production observer 对 reparse、link、读取失败与 TOCTOU 的统一拒绝结果。
            ReconciliationObservation::Uncertain,
        ] {
            assert_eq!(
                decide_reconciled_commit(observation, &new, Some(&old)),
                Err(SourceWriteError::CommitStateUnknown)
            );
        }
    }

    /// Windows readonly target 的原生 replace failure 必须保留完整 OLD，并由 NamedTempFile 清理本次 temp。
    #[cfg(windows)]
    #[tokio::test]
    async fn windows_readonly_replace_failure_keeps_old_target_and_cleans_temp() {
        let (_directory, supervisor, workspace, root, _paths) = fixture();
        let target = root.join("readonly.txt");
        fs::write(&target, b"OLD complete file").unwrap();
        let original_permissions = fs::metadata(&target).unwrap().permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        fs::set_permissions(&target, readonly_permissions).unwrap();
        let locked = lock_target(
            supervisor.as_ref(),
            &workspace,
            "readonly.txt",
            b"OLD complete file",
        )
        .await;

        assert_eq!(
            replace_existing_file(&locked, b"NEW complete file"),
            Err(SourceWriteError::IoError)
        );
        assert_eq!(fs::read(&target).unwrap(), b"OLD complete file");
        assert!(replace_temp_paths(&root).is_empty());
        fs::set_permissions(target, original_permissions).unwrap();
    }

    /// Windows 独占 target handle 的真实 sharing violation 必须保持 OLD，且失败不能留下本次 temp。
    #[cfg(windows)]
    #[tokio::test]
    async fn windows_exclusive_target_handle_failure_keeps_old_and_cleans_temp() {
        use std::os::windows::fs::OpenOptionsExt;

        let (_directory, supervisor, workspace, root, _paths) = fixture();
        let target = root.join("exclusive-handle.txt");
        fs::write(&target, b"OLD complete file").unwrap();
        let locked = lock_target(
            supervisor.as_ref(),
            &workspace,
            "exclusive-handle.txt",
            b"OLD complete file",
        )
        .await;
        // share_mode(0) 是 Windows 文件系统的真实 sharing denial，不是 production fault injection。
        let held = File::options()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&target)
            .unwrap();

        assert_eq!(
            replace_existing_file(&locked, b"NEW complete file"),
            Err(SourceWriteError::IoError)
        );
        drop(held);
        assert_eq!(fs::read(&target).unwrap(), b"OLD complete file");
        assert!(replace_temp_paths(&root).is_empty());
    }

    /// pre-replace child crash 只能留下 non-target orphan temp；target 必须仍是完整 OLD，后续正常写不得被它阻塞。
    #[tokio::test]
    async fn child_pre_replace_crash_keeps_old_and_orphan_does_not_block_next_replace() {
        let (_directory, supervisor, workspace, root, paths) = fixture();
        let target = root.join("crash.txt");
        fs::write(&target, b"OLD complete file").unwrap();

        let status = run_crash_child(&paths, &root, "before-replace");
        assert_eq!(status.code(), Some(CRASH_EXIT_CODE));
        assert_eq!(fs::read(&target).unwrap(), b"OLD complete file");
        assert_eq!(replace_temp_paths(&root).len(), 1);

        let locked = lock_target(
            supervisor.as_ref(),
            &workspace,
            "crash.txt",
            b"OLD complete file",
        )
        .await;
        replace_existing_file(&locked, b"NEW complete file").unwrap();
        assert_eq!(fs::read(target).unwrap(), b"NEW complete file");
    }

    /// post-replace child crash 发生在 parent sync/返回前；reopen target 仍必须是完整 NEW。
    #[tokio::test]
    async fn child_post_replace_crash_keeps_new_complete() {
        let (_directory, _supervisor, _workspace, root, paths) = fixture();
        let target = root.join("crash.txt");
        fs::write(&target, b"OLD complete file").unwrap();

        let status = run_crash_child(&paths, &root, "after-replace");
        assert_eq!(status.code(), Some(CRASH_EXIT_CODE));
        assert_eq!(fs::read(target).unwrap(), b"NEW complete file");
        assert!(replace_temp_paths(&root).is_empty());
    }

    /// helper 成功后 LockedTargetCommit 仍持有 WorkspaceWriteGuard，Remove 必须保持 WORKSPACE_IN_USE 直到 caller drop。
    #[tokio::test]
    async fn replace_keeps_workspace_remove_blocked_until_locked_commit_drops() {
        let (directory, supervisor, workspace, root, _paths) = fixture();
        let target = root.join("remove.txt");
        fs::write(&target, b"OLD complete file").unwrap();
        let store = StateStore::open(directory.path().join("agent-state"))
            .await
            .unwrap();
        let product = AgentProductService::new(store);
        let locked = lock_target(
            supervisor.as_ref(),
            &workspace,
            "remove.txt",
            b"OLD complete file",
        )
        .await;

        replace_existing_file(&locked, b"NEW complete file").unwrap();

        assert_eq!(
            supervisor
                .remove_workspace_coordinated(&product, &workspace.id)
                .await,
            Err(WORKSPACE_IN_USE.into())
        );
        drop(locked);
        assert_eq!(
            supervisor
                .remove_workspace_coordinated(&product, &workspace.id)
                .await
                .unwrap(),
            workspace
        );
    }
}
