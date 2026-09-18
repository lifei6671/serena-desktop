//! Source Write 的 per-target commit 协调与锁内 revalidation；本模块绝不写入目标文件。
#![allow(
    dead_code,
    reason = "P2C-003 establishes the future commit primitive before P2C-004/007 handlers consume it."
)]

use super::source_write_domain::{ExpectedSha256, SourceWriteError, TARGET_TEXT_FILE_MAX};
use crate::{
    config::same_workspace_root_identity,
    serena::{SupervisorState, WorkspaceWriteGuard},
    workspace_path::WorkspacePathResolver,
    workspace_resolver::{WorkspaceLease, WorkspaceResolver},
};
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, Metadata},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

/// 仅用于 lock 内重验的稳定文件对象 identity，绝不进入 wire 或公开 DTO。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    volume_serial: u32,
    #[cfg(windows)]
    file_index: u64,
}

/// 路径或已打开句柄在单个时刻看到的版本证据，identity 外保留长度与 mtime 作为附加检测。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileSnapshot {
    identity: FileIdentity,
    len: u64,
    modified: Option<SystemTime>,
}

/// Supervisor 内部使用的 Workspace authority 错误与冻结 Source Write 错误的区分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetCommitError {
    WorkspaceChanged,
    Source(SourceWriteError),
}

impl TargetCommitError {
    /// 返回后续 adapter 可投影的稳定错误码，但本模块不处理 MCP wire。
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::WorkspaceChanged => "WORKSPACE_CHANGED",
            Self::Source(error) => error.code(),
        }
    }
}

impl From<SourceWriteError> for TargetCommitError {
    /// 保持 P2C-001 Source Write 错误原样，不把 Workspace error 塞入其 enum。
    fn from(error: SourceWriteError) -> Self {
        Self::Source(error)
    }
}

/// 用 canonical existing parent 与平台一致的 final filename 表示唯一逻辑目标。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TargetCommitKey {
    canonical_parent: PathBuf,
    normalized_final_name: OsString,
}

/// 单一 target 的 async mutex 与所有 holder/waiter 共享的生命周期计数。
struct TargetCommitEntry {
    mutex: Arc<AsyncMutex<()>>,
    ref_count: u32,
}

/// 只短暂保护 key table 的状态；实际等待永远发生在每个 target 自己的 async mutex 上。
struct TargetCommitCoordinatorState {
    entries: Mutex<Vec<(TargetCommitKey, TargetCommitEntry)>>,
    #[cfg(test)]
    hash_read_hook: Mutex<Option<HashReadHook>>,
}

/// 仅测试在同一打开句柄的两次 metadata 之间模拟外部变更，不进入生产 API。
#[cfg(test)]
type HashReadHook = Arc<dyn Fn(&Path) + Send + Sync>;

impl Default for TargetCommitCoordinatorState {
    /// 初始化空 lock table；测试 hook 默认关闭。
    fn default() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            #[cfg(test)]
            hash_read_hook: Mutex::new(None),
        }
    }
}

/// Supervisor-owned 的进程内 keyed commit coordinator，不是 global/static 锁表。
#[derive(Clone, Default)]
pub(crate) struct TargetCommitCoordinator {
    state: Arc<TargetCommitCoordinatorState>,
}

impl TargetCommitCoordinator {
    /// 创建一个仅属于单个 Supervisor 的空 target lock table。
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 登记 holder/waiter 后异步等待该 target；future 取消时 Permit Drop 会回收计数。
    async fn acquire(&self, key: TargetCommitKey) -> LockedTargetPermit {
        let mutex = {
            let mut entries = self
                .state
                .entries
                .lock()
                .expect("target commit coordinator mutex poisoned");
            let entry = if let Some((_, entry)) = entries
                .iter_mut()
                .find(|(existing, _)| same_target_key(existing, &key))
            {
                entry
            } else {
                entries.push((
                    key.clone(),
                    TargetCommitEntry {
                        mutex: Arc::new(AsyncMutex::new(())),
                        ref_count: 0,
                    },
                ));
                &mut entries
                    .last_mut()
                    .expect("target commit entry was inserted")
                    .1
            };
            entry.ref_count = entry
                .ref_count
                .checked_add(1)
                .expect("target commit refcount overflow");
            Arc::clone(&entry.mutex)
        };
        let permit = TargetCommitPermit {
            state: Arc::clone(&self.state),
            key,
        };
        let guard = mutex.lock_owned().await;
        LockedTargetPermit { guard, permit }
    }

    /// 仅测试观察 key table 是否随最后一个 holder/waiter 回收。
    #[cfg(test)]
    fn entry_count_for_test(&self) -> usize {
        self.state
            .entries
            .lock()
            .expect("target commit coordinator mutex poisoned")
            .len()
    }

    /// 仅测试观察所有已登记 holder/waiter，供 handler 精确确认已进入同一 target 的锁等待窗口。
    #[cfg(test)]
    pub(crate) fn permit_count_for_test(&self) -> u32 {
        self.state
            .entries
            .lock()
            .expect("target commit coordinator mutex poisoned")
            .iter()
            .map(|(_, entry)| entry.ref_count)
            .sum()
    }

    /// 仅测试设置一次性 hash read hook，以确定性覆盖读取期间版本漂移。
    #[cfg(test)]
    fn set_hash_read_hook_for_test(&self, hook: HashReadHook) {
        *self
            .state
            .hash_read_hook
            .lock()
            .expect("target commit hash hook mutex poisoned") = Some(hook);
    }

    /// 仅测试在 hash 读取开始前取走 hook，避免后续调用意外重复执行。
    #[cfg(test)]
    fn run_hash_read_hook_for_test(&self, target: &Path) {
        if let Some(hook) = self
            .state
            .hash_read_hook
            .lock()
            .expect("target commit hash hook mutex poisoned")
            .take()
        {
            hook(target);
        }
    }
}

/// 释放时递减 holder/waiter 计数；最后一个离开时只删除同一 Arc 对应的 entry。
struct TargetCommitPermit {
    state: Arc<TargetCommitCoordinatorState>,
    key: TargetCommitKey,
}

impl Drop for TargetCommitPermit {
    /// 无论等待 future 被取消还是 holder 正常释放，都必须最终清理 key entry。
    fn drop(&mut self) {
        let mut entries = self
            .state
            .entries
            .lock()
            .expect("target commit coordinator mutex poisoned");
        let Some(index) = entries
            .iter()
            .position(|(key, _)| same_target_key(key, &self.key))
        else {
            debug_assert!(false, "target commit permit must retain its entry");
            return;
        };
        let entry = &mut entries[index].1;
        if entry.ref_count == 1 {
            entries.remove(index);
        } else {
            entry.ref_count -= 1;
        }
    }
}

/// 被成功获取的 per-target mutex ownership；Permit 与 mutex guard 必须共同存活。
struct LockedTargetPermit {
    guard: OwnedMutexGuard<()>,
    permit: TargetCommitPermit,
}

/// P2C-004/007 将在其存活期内提交的已锁定 target；本类型不提供任何写 API。
pub(crate) struct LockedTargetCommit {
    canonical_target: PathBuf,
    before_sha256: Option<ExpectedSha256>,
    _target_lock: LockedTargetPermit,
    _workspace_write_guard: WorkspaceWriteGuard,
}

impl LockedTargetCommit {
    /// 返回锁内再次验证后的 canonical target，仅供后续 commit 层读取。
    pub(crate) fn canonical_target(&self) -> &Path {
        &self.canonical_target
    }

    /// 返回锁内验证成功的原始 SHA token；vacant target 没有 before version。
    pub(crate) fn before_sha256(&self) -> Option<&ExpectedSha256> {
        self.before_sha256.as_ref()
    }
}

/// 为已有普通文件获取 commit lock，并在锁内重新验证 authority、path identity 与 raw SHA。
pub(crate) async fn lock_existing_target(
    supervisor: &SupervisorState,
    lease: WorkspaceLease,
    guard: WorkspaceWriteGuard,
    relative_path: &str,
    expected_sha256: ExpectedSha256,
) -> Result<LockedTargetCommit, TargetCommitError> {
    if !supervisor.workspace_write_guard_matches(&guard, &lease) {
        return Err(TargetCommitError::WorkspaceChanged);
    }
    let pre_target = resolve_workspace_path(&lease, relative_path, false)?;
    ensure_regular_file(&pre_target, false)?;
    let key = key_for_existing_target(&pre_target)?;
    let coordinator = supervisor.target_commit_coordinator();
    let locked_target = coordinator.acquire(key.clone()).await;

    let current_lease = revalidate_workspace(supervisor, &lease, &guard)?;
    let target = resolve_workspace_path(&current_lease, relative_path, true)?;
    if !target.exists() {
        return Err(SourceWriteError::VersionConflict.into());
    }
    if key_for_existing_target(&target)? != key {
        return Err(TargetCommitError::WorkspaceChanged);
    }
    ensure_regular_file(&target, true)?;
    let before_sha256 = hash_regular_file(&coordinator, &target)?;
    if before_sha256 != expected_sha256 {
        return Err(SourceWriteError::VersionConflict.into());
    }

    Ok(LockedTargetCommit {
        canonical_target: target,
        before_sha256: Some(before_sha256),
        _target_lock: locked_target,
        _workspace_write_guard: guard,
    })
}

/// 为不存在且 parent 已存在的 target 获取同一 keyed primitive，并在锁内确认它仍为空。
pub(crate) async fn lock_vacant_target(
    supervisor: &SupervisorState,
    lease: WorkspaceLease,
    guard: WorkspaceWriteGuard,
    relative_path: &str,
) -> Result<LockedTargetCommit, TargetCommitError> {
    if !supervisor.workspace_write_guard_matches(&guard, &lease) {
        return Err(TargetCommitError::WorkspaceChanged);
    }
    let pre_target = resolve_workspace_path(&lease, relative_path, false)?;
    if pre_target.exists() {
        return Err(SourceWriteError::AlreadyExists.into());
    }
    let key = key_for_vacant_target(&pre_target)?;
    let locked_target = supervisor
        .target_commit_coordinator()
        .acquire(key.clone())
        .await;

    let current_lease = revalidate_workspace(supervisor, &lease, &guard)?;
    let target = resolve_workspace_path(&current_lease, relative_path, true)?;
    if target.exists() {
        return Err(SourceWriteError::AlreadyExists.into());
    }
    if key_for_vacant_target(&target)? != key {
        return Err(TargetCommitError::WorkspaceChanged);
    }

    Ok(LockedTargetCommit {
        canonical_target: target,
        before_sha256: None,
        _target_lock: locked_target,
        _workspace_write_guard: guard,
    })
}

/// 重新解析当前 Workspace，确认 captured authority 和 Guard ownership 均未漂移。
fn revalidate_workspace(
    supervisor: &SupervisorState,
    captured_lease: &WorkspaceLease,
    guard: &WorkspaceWriteGuard,
) -> Result<WorkspaceLease, TargetCommitError> {
    let current = WorkspaceResolver::new(supervisor)
        .resolve(&captured_lease.workspace_id)
        .map_err(|_| TargetCommitError::WorkspaceChanged)?;
    if current.workspace_id != captured_lease.workspace_id
        || current.generation != captured_lease.generation
        || !same_workspace_root_identity(&current.canonical_root, &captured_lease.canonical_root)
        || !supervisor.workspace_write_guard_matches(guard, &current)
    {
        return Err(TargetCommitError::WorkspaceChanged);
    }
    Ok(current)
}

/// 单一 WorkspacePathResolver 的错误投影；锁内 authority 失效一律 fail closed 为 WORKSPACE_CHANGED。
fn resolve_workspace_path(
    lease: &WorkspaceLease,
    relative_path: &str,
    locked_revalidation: bool,
) -> Result<PathBuf, TargetCommitError> {
    WorkspacePathResolver::new(lease)
        .resolve(relative_path)
        .map_err(|_| {
            if locked_revalidation {
                TargetCommitError::WorkspaceChanged
            } else {
                SourceWriteError::PathOutsideWorkspace.into()
            }
        })
}

/// 以 canonical existing target 的 parent/name 生成 key，避免使用 caller relative path 作为 key。
fn key_for_existing_target(target: &Path) -> Result<TargetCommitKey, TargetCommitError> {
    let parent = target.parent().ok_or(SourceWriteError::IoError)?;
    let canonical_parent = fs::canonicalize(parent).map_err(|_| SourceWriteError::IoError)?;
    let name = target.file_name().ok_or(SourceWriteError::IoError)?;
    key_from_parent_and_name(canonical_parent, name)
}

/// 以已存在的 canonical parent 与 final filename 生成 vacant key，且绝不创建 parent。
fn key_for_vacant_target(target: &Path) -> Result<TargetCommitKey, TargetCommitError> {
    let parent = target.parent().ok_or(SourceWriteError::NotFound)?;
    let canonical_parent = fs::canonicalize(parent).map_err(|error| match error.kind() {
        ErrorKind::NotFound => SourceWriteError::NotFound,
        _ => SourceWriteError::IoError,
    })?;
    if !canonical_parent.is_dir() {
        return Err(SourceWriteError::NotFound.into());
    }
    let name = target.file_name().ok_or(SourceWriteError::NotFound)?;
    key_from_parent_and_name(canonical_parent, name)
}

/// 统一 existing/vacant 的 key 格式；filename 已由唯一 resolver 完成路径归一化。
fn key_from_parent_and_name(
    canonical_parent: PathBuf,
    name: &OsStr,
) -> Result<TargetCommitKey, TargetCommitError> {
    if name.is_empty() {
        return Err(SourceWriteError::PathOutsideWorkspace.into());
    }
    Ok(TargetCommitKey {
        canonical_parent,
        normalized_final_name: name.to_os_string(),
    })
}

/// 以现有 Workspace root identity 和平台文件名语义比较 key，避免 Windows Unicode case alias 绕过锁。
fn same_target_key(left: &TargetCommitKey, right: &TargetCommitKey) -> bool {
    same_workspace_root_identity(&left.canonical_parent, &right.canonical_parent)
        && same_final_name_identity(&left.normalized_final_name, &right.normalized_final_name)
}

/// Unix 采用精确 filename；Windows 与 WorkspacePathResolver 一样采用 ordinal ignore-case 比较。
#[cfg(not(windows))]
fn same_final_name_identity(left: &OsStr, right: &OsStr) -> bool {
    left == right
}

/// Windows filename identity 不能用 Unicode lowercase 近似，必须复用 ordinal ignore-case 规则。
#[cfg(windows)]
fn same_final_name_identity(left: &OsStr, right: &OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

    let left = left.encode_wide().collect::<Vec<_>>();
    let right = right.encode_wide().collect::<Vec<_>>();
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return false;
    };
    unsafe {
        CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) == CSTR_EQUAL
    }
}

/// 确认 target 仍是普通文件；锁内消失是版本竞争，其他异常保持 P2C-001 IO 分类。
fn ensure_regular_file(target: &Path, locked_revalidation: bool) -> Result<(), TargetCommitError> {
    match fs::metadata(target) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(if locked_revalidation {
            SourceWriteError::VersionConflict
        } else {
            SourceWriteError::NotFound
        }
        .into()),
        Err(error) if error.kind() == ErrorKind::NotFound => Err(if locked_revalidation {
            SourceWriteError::VersionConflict
        } else {
            SourceWriteError::NotFound
        }
        .into()),
        Err(_) => Err(SourceWriteError::IoError.into()),
    }
}

/// 流式 raw SHA-256，并以 path/handle 双 snapshot 检测 external edit 或 atomic replace。
fn hash_regular_file(
    coordinator: &TargetCommitCoordinator,
    target: &Path,
) -> Result<ExpectedSha256, TargetCommitError> {
    #[cfg(not(test))]
    let _ = coordinator;
    let path_before = path_snapshot(target)?;
    let mut file = File::open(target).map_err(|error| match error.kind() {
        ErrorKind::NotFound => SourceWriteError::VersionConflict,
        _ => SourceWriteError::IoError,
    })?;
    let before = handle_snapshot(&file)?;
    if !same_file_snapshot(&path_before, &before) {
        return Err(SourceWriteError::VersionConflict.into());
    }
    if path_before.len > TARGET_TEXT_FILE_MAX as u64 || before.len > TARGET_TEXT_FILE_MAX as u64 {
        return Err(SourceWriteError::FileTooLarge.into());
    }
    #[cfg(test)]
    coordinator.run_hash_read_hook_for_test(target);

    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut read_len = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| SourceWriteError::IoError)?;
        if count == 0 {
            break;
        }
        read_len = read_len
            .checked_add(count as u64)
            .ok_or(SourceWriteError::FileTooLarge)?;
        if read_len > TARGET_TEXT_FILE_MAX as u64 {
            return Err(SourceWriteError::FileTooLarge.into());
        }
        hasher.update(&buffer[..count]);
    }
    let after = handle_snapshot(&file)?;
    let path_after = path_snapshot(target)?;
    if read_len != before.len
        || !same_file_snapshot(&before, &after)
        || !same_file_snapshot(&path_before, &path_after)
        || !same_file_snapshot(&before, &path_after)
    {
        return Err(SourceWriteError::VersionConflict.into());
    }
    let digest = hasher.finalize();
    let encoded = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    ExpectedSha256::parse(encoded).map_err(TargetCommitError::from)
}

/// 路径 snapshot 先拒绝 link/reparse，再以独立打开句柄取得稳定 identity。
pub(crate) fn path_snapshot(target: &Path) -> Result<FileSnapshot, TargetCommitError> {
    let metadata = fs::symlink_metadata(target).map_err(|error| {
        TargetCommitError::from(match error.kind() {
            ErrorKind::NotFound => SourceWriteError::VersionConflict,
            _ => SourceWriteError::IoError,
        })
    })?;
    ensure_plain_target(&metadata)?;
    let file = File::open(target).map_err(|error| match error.kind() {
        ErrorKind::NotFound => SourceWriteError::VersionConflict,
        _ => SourceWriteError::IoError,
    })?;
    let snapshot = handle_snapshot(&file)?;
    if !same_file_snapshot_metadata(&snapshot, &metadata) {
        return Err(SourceWriteError::VersionConflict.into());
    }
    Ok(snapshot)
}

/// 读取已打开句柄的 metadata 与 platform identity；失败意味着 revalidation 无法可靠继续。
pub(crate) fn handle_snapshot(file: &File) -> Result<FileSnapshot, TargetCommitError> {
    let metadata = file
        .metadata()
        .map_err(|_| TargetCommitError::from(SourceWriteError::VersionConflict))?;
    if !metadata.is_file() {
        return Err(SourceWriteError::VersionConflict.into());
    }
    Ok(FileSnapshot {
        identity: file_identity(file, &metadata)?,
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

/// 路径 metadata 必须仍代表与其临时 handle 相同的长度/mtime，避免 link/reparse 检查后的替换窗口。
fn same_file_snapshot_metadata(snapshot: &FileSnapshot, metadata: &Metadata) -> bool {
    snapshot.len == metadata.len() && same_modified(snapshot.modified, metadata.modified().ok())
}

/// 拒绝 hash 窗口内出现的 symlink 或 Windows reparse file；这不是第二套 Workspace path resolver。
fn ensure_plain_target(metadata: &Metadata) -> Result<(), TargetCommitError> {
    if !metadata.is_file() || metadata.file_type().is_symlink() || is_reparse_point(metadata) {
        return Err(SourceWriteError::VersionConflict.into());
    }
    Ok(())
}

/// Windows reparse point 属性补充 file_type 的 symlink 标识；其他平台无需额外属性。
#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & 0x400 != 0
}

/// Unix file type 已表达 symlink；其余 reparse 概念不适用。
#[cfg(not(windows))]
fn is_reparse_point(_metadata: &Metadata) -> bool {
    false
}

/// Unix/macOS 使用设备号与 inode 作为稳定的打开文件 identity。
#[cfg(unix)]
fn file_identity(_file: &File, metadata: &Metadata) -> Result<FileIdentity, TargetCommitError> {
    use std::os::unix::fs::MetadataExt;

    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

/// Windows 使用打开 handle 的 volume serial 与 file index；标准 MetadataExt 未暴露该 identity。
#[cfg(windows)]
fn file_identity(file: &File, _metadata: &Metadata) -> Result<FileIdentity, TargetCommitError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
    };

    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    let success =
        unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info as *mut _) };
    if success == 0 {
        return Err(SourceWriteError::VersionConflict.into());
    }
    Ok(FileIdentity {
        volume_serial: info.dwVolumeSerialNumber,
        file_index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    })
}

/// identity、长度或可用 modified timestamp 任一改变即认定外部版本发生漂移。
pub(crate) fn same_file_snapshot(before: &FileSnapshot, after: &FileSnapshot) -> bool {
    before.identity == after.identity
        && before.len == after.len
        && same_modified(before.modified, after.modified)
}

/// 仅在两次都能读取 timestamp 时比较；长度检查仍覆盖可观测的增长或截断。
fn same_modified(before: Option<SystemTime>, after: Option<SystemTime>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => before == after,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::{product::AgentProductService, store::StateStore},
        config::{self, AppPaths, ManagerConfig, Workspace},
        workspace_registry::{WORKSPACE_IN_USE, WorkspaceRegistry},
    };
    use std::{sync::Arc, time::Duration};

    /// 建立含一个普通 Workspace 与两个可独立 target 的最小 commit fixture。
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

    /// 计算测试输入的冻结 raw SHA-256 token。
    fn expected_sha256(bytes: &[u8]) -> ExpectedSha256 {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let encoded = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        ExpectedSha256::parse(encoded).unwrap()
    }

    /// 解析本次 writer 专属 Lease 与 Remove-exclusion Guard。
    fn writer(
        supervisor: &SupervisorState,
        workspace: &Workspace,
    ) -> (WorkspaceLease, WorkspaceWriteGuard) {
        supervisor
            .resolve_workspace_write_guard(&workspace.id)
            .unwrap()
    }

    #[tokio::test]
    /// 同一 target 的 waiter 必须串行，holder/waiter 全部离开后 lock table 不保留历史路径。
    async fn same_target_is_serial_and_entries_are_reclaimed_after_holder_waiter_and_cancel() {
        let (_directory, supervisor, workspace, root) = fixture();
        fs::write(root.join("same.txt"), b"A").unwrap();
        let expected = expected_sha256(b"A");
        let (first_lease, first_guard) = writer(supervisor.as_ref(), &workspace);
        let first = lock_existing_target(
            supervisor.as_ref(),
            first_lease,
            first_guard,
            "same.txt",
            expected.clone(),
        )
        .await
        .unwrap();
        let (second_lease, second_guard) = writer(supervisor.as_ref(), &workspace);
        let waiting_supervisor = Arc::clone(&supervisor);
        let mut waiter = tokio::spawn(async move {
            lock_existing_target(
                waiting_supervisor.as_ref(),
                second_lease,
                second_guard,
                "same.txt",
                expected,
            )
            .await
        });

        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut waiter)
                .await
                .is_err()
        );
        drop(first);
        let second = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        drop(second);
        assert_eq!(
            supervisor
                .target_commit_coordinator()
                .entry_count_for_test(),
            0
        );

        let coordinator = supervisor.target_commit_coordinator();
        let key = TargetCommitKey {
            canonical_parent: fs::canonicalize(root).unwrap(),
            normalized_final_name: "cancel.txt".into(),
        };
        let holder = coordinator.acquire(key.clone()).await;
        let mut cancelled_waiter = Box::pin(coordinator.acquire(key));
        tokio::select! {
            biased;
            _ = &mut cancelled_waiter => panic!("waiting target lock must not enter before holder drop"),
            _ = tokio::task::yield_now() => {}
        }
        drop(cancelled_waiter);
        drop(holder);
        assert_eq!(coordinator.entry_count_for_test(), 0);
    }

    #[tokio::test]
    /// 不同 canonical target 的 holder 可同时进入，证明 coordinator 不是全 Workspace mutex。
    async fn different_targets_can_hold_commit_locks_concurrently() {
        let (_directory, supervisor, workspace, root) = fixture();
        fs::write(root.join("left.txt"), b"A").unwrap();
        fs::write(root.join("right.txt"), b"B").unwrap();
        let (left_lease, left_guard) = writer(supervisor.as_ref(), &workspace);
        let left = lock_existing_target(
            supervisor.as_ref(),
            left_lease,
            left_guard,
            "left.txt",
            expected_sha256(b"A"),
        )
        .await
        .unwrap();
        let (right_lease, right_guard) = writer(supervisor.as_ref(), &workspace);
        let right = tokio::time::timeout(
            Duration::from_millis(100),
            lock_existing_target(
                supervisor.as_ref(),
                right_lease,
                right_guard,
                "right.txt",
                expected_sha256(b"B"),
            ),
        )
        .await
        .expect("different targets must not wait on one Workspace-wide mutex")
        .unwrap();
        drop((left, right));
        assert_eq!(
            supervisor
                .target_commit_coordinator()
                .entry_count_for_test(),
            0
        );
    }

    #[tokio::test]
    /// 两个同 SHA writer 只能有第一个继续到未来 commit primitive；第二个必须看到版本冲突。
    async fn same_target_occ_race_allows_only_one_writer_to_revalidate_expected_sha() {
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("race.txt");
        fs::write(&target, b"A").unwrap();
        let expected = expected_sha256(b"A");
        let (first_lease, first_guard) = writer(supervisor.as_ref(), &workspace);
        let first = lock_existing_target(
            supervisor.as_ref(),
            first_lease,
            first_guard,
            "race.txt",
            expected.clone(),
        )
        .await
        .unwrap();
        let (second_lease, second_guard) = writer(supervisor.as_ref(), &workspace);
        let waiting_supervisor = Arc::clone(&supervisor);
        let second = tokio::spawn(async move {
            lock_existing_target(
                waiting_supervisor.as_ref(),
                second_lease,
                second_guard,
                "race.txt",
                expected,
            )
            .await
        });
        tokio::task::yield_now().await;
        // 测试仅模拟 P2C-004 的未来完整 commit；生产 coordinator 没有写入路径。
        fs::write(&target, b"B").unwrap();
        drop(first);
        assert!(matches!(
            second.await.unwrap(),
            Err(TargetCommitError::Source(SourceWriteError::VersionConflict))
        ));
        assert_eq!(
            supervisor
                .target_commit_coordinator()
                .entry_count_for_test(),
            0
        );
    }

    #[tokio::test]
    /// 锁前或等待期间的外部 content edit 必须在锁内 raw SHA revalidation 时失败。
    async fn external_edit_is_a_source_version_conflict() {
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("external.txt");
        fs::write(&target, b"A").unwrap();
        let (lease, guard) = writer(supervisor.as_ref(), &workspace);
        fs::write(target, b"B").unwrap();
        assert!(matches!(
            lock_existing_target(
                supervisor.as_ref(),
                lease,
                guard,
                "external.txt",
                expected_sha256(b"A"),
            )
            .await,
            Err(TargetCommitError::Source(SourceWriteError::VersionConflict))
        ));
    }

    #[tokio::test]
    /// 锁内 hash 必须先按 metadata 拒绝超过 8 MiB 的外部膨胀目标，避免无界读取。
    async fn oversized_target_is_rejected_before_streaming_hash() {
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("large.bin");
        fs::write(&target, vec![0_u8; TARGET_TEXT_FILE_MAX + 1]).unwrap();
        let (lease, guard) = writer(supervisor.as_ref(), &workspace);
        assert!(matches!(
            lock_existing_target(
                supervisor.as_ref(),
                lease,
                guard,
                "large.bin",
                expected_sha256(b"unused"),
            )
            .await,
            Err(TargetCommitError::Source(SourceWriteError::FileTooLarge))
        ));
    }

    #[tokio::test]
    /// 同一打开句柄 hash 期间被测试 hook 改变时，metadata snapshot 检查必须拒绝继续 commit。
    async fn metadata_change_during_hash_is_a_source_version_conflict() {
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("metadata.txt");
        fs::write(&target, b"AAAA").unwrap();
        supervisor
            .target_commit_coordinator()
            .set_hash_read_hook_for_test(Arc::new(|path| {
                // 测试 hook 模拟 hash 读取中的外部完整替换；不会进入生产代码。
                fs::write(path, b"BBBBB").unwrap();
            }));
        let (lease, guard) = writer(supervisor.as_ref(), &workspace);
        assert!(matches!(
            lock_existing_target(
                supervisor.as_ref(),
                lease,
                guard,
                "metadata.txt",
                expected_sha256(b"AAAA"),
            )
            .await,
            Err(TargetCommitError::Source(SourceWriteError::VersionConflict))
        ));
    }

    #[tokio::test]
    /// 同长度且保持 mtime 的 atomic replace 仍必须由 file identity 识别为版本冲突。
    async fn atomic_replace_during_hash_is_a_source_version_conflict() {
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("replace.txt");
        let replacement = root.join("replacement.txt");
        fs::write(&target, b"AAAA").unwrap();
        let original = path_snapshot(&target).unwrap();
        fs::write(&replacement, b"BBBB").unwrap();
        File::options()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(original.modified.unwrap()))
            .unwrap();
        let replacement_snapshot = path_snapshot(&replacement).unwrap();
        assert_eq!(replacement_snapshot.len, original.len);
        assert_eq!(replacement_snapshot.modified, original.modified);
        assert_ne!(replacement_snapshot.identity, original.identity);
        supervisor
            .target_commit_coordinator()
            .set_hash_read_hook_for_test(Arc::new(move |path| {
                // 测试 hook 模拟外部 atomic replace；不会进入生产 coordinator API。
                fs::rename(&replacement, path).unwrap();
            }));
        let (lease, guard) = writer(supervisor.as_ref(), &workspace);
        assert!(matches!(
            lock_existing_target(
                supervisor.as_ref(),
                lease,
                guard,
                "replace.txt",
                expected_sha256(b"AAAA"),
            )
            .await,
            Err(TargetCommitError::Source(SourceWriteError::VersionConflict))
        ));
    }

    /// Windows junction 在 pre-lock 后改向 Workspace 外时，waiter 的锁内 resolver 必须拒绝 authority 漂移。
    #[cfg(windows)]
    #[tokio::test]
    async fn windows_junction_escape_after_prelock_is_workspace_changed() {
        let (_directory, supervisor, workspace, root) = fixture();
        let inside = root.join("inside");
        let outside = root.parent().unwrap().join("outside");
        let link = root.join("link");
        fs::create_dir(&inside).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(inside.join("target.txt"), b"A").unwrap();
        create_junction(&link, &inside);

        let (first_lease, first_guard) = writer(supervisor.as_ref(), &workspace);
        let first = lock_existing_target(
            supervisor.as_ref(),
            first_lease,
            first_guard,
            "link/target.txt",
            expected_sha256(b"A"),
        )
        .await
        .unwrap();
        let (second_lease, second_guard) = writer(supervisor.as_ref(), &workspace);
        let waiting_supervisor = Arc::clone(&supervisor);
        let second = tokio::spawn(async move {
            lock_existing_target(
                waiting_supervisor.as_ref(),
                second_lease,
                second_guard,
                "link/target.txt",
                expected_sha256(b"A"),
            )
            .await
        });
        tokio::task::yield_now().await;
        fs::remove_dir(&link).unwrap();
        create_junction(&link, &outside);
        drop(first);
        assert!(matches!(
            second.await.unwrap(),
            Err(TargetCommitError::WorkspaceChanged)
        ));
    }

    /// Unix symbolic link 的等价逃逸同样必须由锁内 WorkspacePathResolver 拒绝。
    #[cfg(unix)]
    #[tokio::test]
    async fn unix_symlink_escape_after_prelock_is_workspace_changed() {
        use std::os::unix::fs::symlink;

        let (_directory, supervisor, workspace, root) = fixture();
        let inside = root.join("inside");
        let outside = root.parent().unwrap().join("outside");
        let link = root.join("link");
        fs::create_dir(&inside).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(inside.join("target.txt"), b"A").unwrap();
        symlink(&inside, &link).unwrap();

        let (first_lease, first_guard) = writer(supervisor.as_ref(), &workspace);
        let first = lock_existing_target(
            supervisor.as_ref(),
            first_lease,
            first_guard,
            "link/target.txt",
            expected_sha256(b"A"),
        )
        .await
        .unwrap();
        let (second_lease, second_guard) = writer(supervisor.as_ref(), &workspace);
        let waiting_supervisor = Arc::clone(&supervisor);
        let second = tokio::spawn(async move {
            lock_existing_target(
                waiting_supervisor.as_ref(),
                second_lease,
                second_guard,
                "link/target.txt",
                expected_sha256(b"A"),
            )
            .await
        });
        tokio::task::yield_now().await;
        fs::remove_file(&link).unwrap();
        symlink(&outside, &link).unwrap();
        drop(first);
        assert!(matches!(
            second.await.unwrap(),
            Err(TargetCommitError::WorkspaceChanged)
        ));
    }

    /// 创建 Windows junction 的测试辅助，只操作 fixture 目录。
    #[cfg(windows)]
    fn create_junction(link: &Path, destination: &Path) {
        let output = std::process::Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(destination)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "mklink failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    /// Lease generation 漂移、跨 Supervisor Guard 或 path authority 漂移都必须 fail closed。
    async fn authority_and_guard_mismatches_are_workspace_changed() {
        let (_directory, supervisor, workspace, root) = fixture();
        fs::write(root.join("authority.txt"), b"A").unwrap();
        let (lease, guard) = writer(supervisor.as_ref(), &workspace);
        WorkspaceRegistry::new(supervisor.as_ref())
            .mutate(|workspaces| {
                workspaces[0].generation += 1;
                Ok(())
            })
            .unwrap();
        assert!(matches!(
            lock_existing_target(
                supervisor.as_ref(),
                lease,
                guard,
                "authority.txt",
                expected_sha256(b"A"),
            )
            .await,
            Err(TargetCommitError::WorkspaceChanged)
        ));

        let (_other_directory, other_supervisor, _other_workspace, _other_root) = fixture();
        let (first_lease, _first_guard) = writer(supervisor.as_ref(), &workspace);
        let (_other_lease, other_guard) = writer(other_supervisor.as_ref(), &workspace);
        assert!(matches!(
            lock_existing_target(
                supervisor.as_ref(),
                first_lease,
                other_guard,
                "authority.txt",
                expected_sha256(b"A"),
            )
            .await,
            Err(TargetCommitError::WorkspaceChanged)
        ));
    }

    #[tokio::test]
    /// rename、reorder 和 Desktop selection 只改展示/默认项，不能让已捕获 Lease 失效。
    async fn presentation_only_registry_changes_do_not_invalidate_commit_authority() {
        let (_directory, supervisor, workspace, root) = fixture();
        fs::write(root.join("stable.txt"), b"A").unwrap();
        let (lease, guard) = writer(supervisor.as_ref(), &workspace);
        let registry = WorkspaceRegistry::new(supervisor.as_ref());
        registry.rename(&workspace.id, "Renamed".into()).unwrap();
        registry.reorder(vec![workspace.id.clone()]).unwrap();
        supervisor.select_desktop_workspace(&workspace.id).unwrap();
        let locked = lock_existing_target(
            supervisor.as_ref(),
            lease,
            guard,
            "stable.txt",
            expected_sha256(b"A"),
        )
        .await
        .unwrap();
        assert_eq!(
            locked.canonical_target(),
            fs::canonicalize(root.join("stable.txt")).unwrap()
        );
        drop(locked);
    }

    #[tokio::test]
    /// 同一 vacant path 的两个 create contender 共用 key；第一个测试创建后第二个返回 AlreadyExists。
    async fn vacant_target_contenders_share_key_and_revalidate_nonexistence() {
        let (_directory, supervisor, workspace, root) = fixture();
        let target = root.join("new.txt");
        let (first_lease, first_guard) = writer(supervisor.as_ref(), &workspace);
        let first = lock_vacant_target(supervisor.as_ref(), first_lease, first_guard, "new.txt")
            .await
            .unwrap();
        let (second_lease, second_guard) = writer(supervisor.as_ref(), &workspace);
        let waiting_supervisor = Arc::clone(&supervisor);
        let second = tokio::spawn(async move {
            lock_vacant_target(
                waiting_supervisor.as_ref(),
                second_lease,
                second_guard,
                "new.txt",
            )
            .await
        });
        tokio::task::yield_now().await;
        // 测试仅模拟后续 create commit；P2C-003 生产代码不会创建目标。
        fs::write(target, b"created").unwrap();
        drop(first);
        assert!(matches!(
            second.await.unwrap(),
            Err(TargetCommitError::Source(SourceWriteError::AlreadyExists))
        ));
        assert_eq!(
            supervisor
                .target_commit_coordinator()
                .entry_count_for_test(),
            0
        );
    }

    #[tokio::test]
    /// 不同 vacant target 同时成功，且缺失 parent 不会被 coordinator 自动创建。
    async fn different_vacant_targets_are_concurrent_and_missing_parent_is_not_created() {
        let (_directory, supervisor, workspace, root) = fixture();
        let (left_lease, left_guard) = writer(supervisor.as_ref(), &workspace);
        let left = lock_vacant_target(supervisor.as_ref(), left_lease, left_guard, "left.txt")
            .await
            .unwrap();
        let (right_lease, right_guard) = writer(supervisor.as_ref(), &workspace);
        let right = tokio::time::timeout(
            Duration::from_millis(100),
            lock_vacant_target(supervisor.as_ref(), right_lease, right_guard, "right.txt"),
        )
        .await
        .expect("different vacant targets must not share a lock")
        .unwrap();
        drop((left, right));

        let (missing_lease, missing_guard) = writer(supervisor.as_ref(), &workspace);
        assert!(matches!(
            lock_vacant_target(
                supervisor.as_ref(),
                missing_lease,
                missing_guard,
                "missing/child.txt",
            )
            .await,
            Err(TargetCommitError::Source(SourceWriteError::NotFound))
        ));
        assert!(!root.join("missing").exists());
    }

    #[tokio::test]
    /// LockedTargetCommit 持有内部 Guard 时 Remove 必须被拒绝，Drop 后才可继续 Remove。
    async fn locked_target_commit_retains_workspace_write_guard_until_drop() {
        let (directory, supervisor, workspace, root) = fixture();
        fs::write(root.join("remove.txt"), b"A").unwrap();
        let store = StateStore::open(directory.path().join("agent-state"))
            .await
            .unwrap();
        let product = AgentProductService::new(store);
        let (lease, guard) = writer(supervisor.as_ref(), &workspace);
        let locked = lock_existing_target(
            supervisor.as_ref(),
            lease,
            guard,
            "remove.txt",
            expected_sha256(b"A"),
        )
        .await
        .unwrap();
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
