use super::{registry::SourceArgs, source_read_support};
use crate::workspace_resolver::WorkspaceLease;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, VecDeque},
    fs,
    path::Path,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const DEFAULT_MAX_BYTES: usize = 65_536;
const MAX_MAX_BYTES: usize = 262_144;
// 防止异常目录树耗尽服务资源；这些只是不影响普通源码树的内部上限，不是公开参数。
const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 64;
const TRAVERSAL_TIMEOUT: Duration = Duration::from_secs(5);

/// 本地目录列表的稳定 JSON 载荷；公开外层仍由 handler 提供 workspace、text、truncated。
#[derive(Default, Serialize)]
struct DirectoryListing {
    dirs: BTreeSet<String>,
    files: BTreeSet<String>,
}

/// 一项待递归的已验证普通目录；绝不把链接或 reparse point 放进队列。
struct PendingDirectory {
    relative_path: String,
    depth: usize,
}

/// 内部硬界集中在一个值对象中，测试可用极小值直接覆盖每个停止分支。
#[derive(Clone, Copy)]
struct TraversalLimits {
    max_entries: usize,
    max_depth: usize,
    timeout: Duration,
}

impl TraversalLimits {
    /// 返回 production traversal 的防病态硬界。
    fn production() -> Self {
        Self {
            max_entries: MAX_ENTRIES,
            max_depth: MAX_DEPTH,
            timeout: TRAVERSAL_TIMEOUT,
        }
    }
}

/// 从已捕获 Lease 枚举目录；不调用 Serena 或 WorkspaceCapabilityManager。
pub(super) async fn list(
    lease: &WorkspaceLease,
    arguments: SourceArgs,
    cancel: CancellationToken,
) -> Result<Value, String> {
    let relative_path = arguments
        .relative_path
        .ok_or("INVALID_PARAMS: 缺少 relative_path")?;
    let max_bytes = max_bytes(arguments.max_bytes)?;
    let recursive = arguments.recursive.unwrap_or(false);
    let lease = lease.clone();
    let response_lease = lease.clone();

    let result = source_read_support::run_blocking_cancellable(cancel, move |worker_cancel| {
        list_with_limits(
            &lease,
            &relative_path,
            recursive,
            max_bytes,
            &worker_cancel,
            TraversalLimits::production(),
        )
    })
    .await?;

    let mut response =
        serde_json::to_value(source_read_support::workspace_provenance(&response_lease))
            .expect("Workspace provenance must serialize");
    response["text"] = json!(result.text);
    response["truncated"] = json!(result.truncated);
    Ok(response)
}

/// 验证继承自 compatibility list_dir 的公开 byte budget。
fn max_bytes(value: Option<usize>) -> Result<usize, String> {
    let value = value.unwrap_or(DEFAULT_MAX_BYTES);
    if value == 0 || value > MAX_MAX_BYTES {
        return Err("INVALID_PARAMS: max_bytes 超出范围".into());
    }
    Ok(value)
}

/// 完成的目录文本及是否因 public budget 或内部硬界而提前停止。
struct ListResult {
    text: String,
    truncated: bool,
}

/// 以唯一 WorkspacePathResolver 验证每个普通目录，再按稳定顺序流式填充结果。
fn list_with_limits(
    lease: &WorkspaceLease,
    relative_path: &str,
    recursive: bool,
    max_bytes: usize,
    cancel: &CancellationToken,
    limits: TraversalLimits,
) -> Result<ListResult, String> {
    list_with_limits_and_hook(
        lease,
        relative_path,
        recursive,
        max_bytes,
        cancel,
        limits,
        |_| {},
    )
}

/// 目录循环的测试 hook 精确控制取消时序；production 始终传入无操作 hook。
fn list_with_limits_and_hook<Hook>(
    lease: &WorkspaceLease,
    relative_path: &str,
    recursive: bool,
    max_bytes: usize,
    cancel: &CancellationToken,
    limits: TraversalLimits,
    mut on_entry: Hook,
) -> Result<ListResult, String>
where
    Hook: FnMut(usize),
{
    source_read_support::check_cancelled(cancel)?;
    let root = source_read_support::workspace_root(lease)?;
    let target = source_read_support::resolve_relative_path(lease, relative_path)?;
    let metadata = fs::metadata(&target).map_err(|_| "INVALID_PATH: target is unavailable")?;
    if !metadata.is_dir() {
        return Err("INVALID_PATH: expected a directory".into());
    }

    let mut listing = DirectoryListing::default();
    let mut truncated = serialized_listing(&listing).len() > max_bytes;
    if truncated {
        return finish_listing(listing, true);
    }
    let target_relative = workspace_relative_path(&root, &target)?;
    let mut pending = VecDeque::from([PendingDirectory {
        relative_path: target_relative,
        depth: 0,
    }]);
    let started = Instant::now();
    let mut entries_seen = 0usize;

    while let Some(current) = pending.pop_front() {
        if should_stop(cancel, started, limits.timeout)? {
            truncated = true;
            break;
        }
        // 重新解析 queue item，防止其在入队后被替换为 root 外的链接目标。
        let directory = source_read_support::resolve_relative_path(lease, &current.relative_path)?;
        let directory_metadata =
            fs::symlink_metadata(&directory).map_err(|_| "INVALID_PATH: target is unavailable")?;
        if is_reparse_point(&directory_metadata) {
            continue;
        }
        // read_dir 保持惰性迭代：到达 budget/hard bound 后不再预扫描整棵巨大目录树。
        let entries =
            fs::read_dir(&directory).map_err(|_| "INVALID_PATH: target could not be listed")?;
        for entry in entries {
            if should_stop(cancel, started, limits.timeout)? {
                truncated = true;
                break;
            }
            if entries_seen >= limits.max_entries {
                truncated = true;
                break;
            }
            entries_seen += 1;
            on_entry(entries_seen);
            source_read_support::check_cancelled(cancel)?;

            let entry =
                entry.map_err(|_| "INVALID_PATH: directory entry could not be inspected")?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|_| "INVALID_PATH: directory entry could not be inspected")?;
            let reparse_point = is_reparse_point(&metadata);
            let is_directory = metadata.is_dir();
            let entry_relative = workspace_relative_path(&root, &path)?;
            if !try_add_entry(
                &mut listing,
                entry_relative.clone(),
                is_directory,
                max_bytes,
            ) {
                truncated = true;
                break;
            }

            // 仅普通目录可递归；symlink/junction/reparse 仅作为可见条目，绝不跟随。
            if recursive && is_directory && !reparse_point {
                if current.depth >= limits.max_depth {
                    truncated = true;
                    break;
                }
                // 将每个普通子目录重新交给 Resolver，防止 entry 在检查后发生边界变化。
                source_read_support::resolve_relative_path(lease, &entry_relative)?;
                pending.push_back(PendingDirectory {
                    relative_path: entry_relative,
                    depth: current.depth + 1,
                });
            }
        }
        if truncated {
            break;
        }
    }
    finish_listing(listing, truncated)
}

/// 在每次 I/O 与每个 entry 前统一检查 cancellation 和内部 deadline。
fn should_stop(
    cancel: &CancellationToken,
    started: Instant,
    timeout: Duration,
) -> Result<bool, String> {
    source_read_support::check_cancelled(cancel)?;
    Ok(started.elapsed() >= timeout)
}

/// 把候选加入相应分组；一旦完整 JSON 会超过预算即撤销该条目并停止扫描。
fn try_add_entry(
    listing: &mut DirectoryListing,
    entry: String,
    is_directory: bool,
    max_bytes: usize,
) -> bool {
    let inserted = if is_directory {
        listing.dirs.insert(entry.clone())
    } else {
        listing.files.insert(entry.clone())
    };
    if !inserted {
        return true;
    }
    if serialized_listing(listing).len() <= max_bytes {
        return true;
    }
    if is_directory {
        listing.dirs.remove(&entry);
    } else {
        listing.files.remove(&entry);
    }
    false
}

/// 序列化是唯一的输出长度度量，保证任何提前停止仍返回完整 JSON object。
fn serialized_listing(listing: &DirectoryListing) -> String {
    serde_json::to_string(listing).expect("Directory listing must serialize")
}

/// 收尾时始终产出完整 JSON，避免按 byte slice 截断 UTF-8 或 JSON token。
fn finish_listing(listing: DirectoryListing, truncated: bool) -> Result<ListResult, String> {
    Ok(ListResult {
        text: serialized_listing(&listing),
        truncated,
    })
}

/// 将已由 Resolver 约束在 root 内的路径投影成当前平台的 Workspace-relative 文本。
fn workspace_relative_path(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "INVALID_PATH: path escapes the workspace root")?;
    if relative.as_os_str().is_empty() {
        return Err("INVALID_PATH: workspace root is not a path target".into());
    }
    relative
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "INVALID_PATH: path is not valid Unicode".into())
}

/// 检测链接与 Windows reparse point，二者都只能列出而不能作为递归目标。
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return metadata.file_attributes() & 0x400 != 0;
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
#[path = "source_list_tests.rs"]
mod tests;
