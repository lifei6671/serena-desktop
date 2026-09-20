use super::{registry::SourceArgs, source_read_support};
use crate::workspace_resolver::WorkspaceLease;
use globset::{GlobBuilder, GlobMatcher};
use ignore::WalkBuilder;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const DEFAULT_MAX_BYTES: usize = 65_536;
const MAX_MAX_BYTES: usize = 262_144;
// 防止异常目录树耗尽服务资源；它们不是公开参数，普通源码树不会触及。
const MAX_ENTRIES: usize = 100_000;
const MAX_MATCHES: usize = 10_000;
const MAX_DEPTH: usize = 64;
const TRAVERSAL_TIMEOUT: Duration = Duration::from_secs(5);

/// 本地文件查找的稳定 JSON 载荷；公开外层仍使用 workspace、text 与 truncated。
#[derive(Default, Serialize)]
struct FileMatches {
    files: BTreeSet<String>,
}

/// 内部硬界集中在一个值对象中，测试可用极小值覆盖每个停止分支。
#[derive(Clone, Copy)]
struct TraversalLimits {
    max_entries: usize,
    max_matches: usize,
    max_depth: usize,
    timeout: Duration,
}

impl TraversalLimits {
    /// 返回 production traversal 的防病态硬界。
    fn production() -> Self {
        Self {
            max_entries: MAX_ENTRIES,
            max_matches: MAX_MATCHES,
            max_depth: MAX_DEPTH,
            timeout: TRAVERSAL_TIMEOUT,
        }
    }
}

/// 从 captured Lease 的 Workspace 查找 basename；不读取文件正文，也不调用 Serena。
pub(super) async fn find(
    lease: &WorkspaceLease,
    arguments: SourceArgs,
    cancel: CancellationToken,
) -> Result<Value, String> {
    let file_mask = arguments
        .file_mask
        .ok_or("INVALID_PARAMS: 缺少 file_mask")?;
    let max_bytes = max_bytes(arguments.max_bytes)?;
    let lease = lease.clone();
    let response_lease = lease.clone();

    let result = source_read_support::run_blocking_cancellable(cancel, move |worker_cancel| {
        find_with_limits(
            &lease,
            arguments.relative_path.as_deref(),
            &file_mask,
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

/// 验证兼容性 find_file 的公开 byte budget。
fn max_bytes(value: Option<usize>) -> Result<usize, String> {
    let value = value.unwrap_or(DEFAULT_MAX_BYTES);
    if value == 0 || value > MAX_MAX_BYTES {
        return Err("INVALID_PARAMS: max_bytes 超出范围".into());
    }
    Ok(value)
}

/// 完成的文件列表文本及是否因公开预算或内部硬界而提前停止。
struct FindResult {
    text: String,
    truncated: bool,
}

/// 以唯一 WorkspacePathResolver 验证搜索 root，并按惰性、受限 traversal 填充结果。
fn find_with_limits(
    lease: &WorkspaceLease,
    relative_path: Option<&str>,
    file_mask: &str,
    max_bytes: usize,
    cancel: &CancellationToken,
    limits: TraversalLimits,
) -> Result<FindResult, String> {
    find_with_limits_and_hook(
        lease,
        relative_path,
        file_mask,
        max_bytes,
        cancel,
        limits,
        |_| {},
    )
}

/// traversal hook 仅用于测试精确控制取消时序；production 始终传入无操作 hook。
fn find_with_limits_and_hook<Hook>(
    lease: &WorkspaceLease,
    relative_path: Option<&str>,
    file_mask: &str,
    max_bytes: usize,
    cancel: &CancellationToken,
    limits: TraversalLimits,
    mut on_entry: Hook,
) -> Result<FindResult, String>
where
    Hook: FnMut(usize),
{
    source_read_support::check_cancelled(cancel)?;
    let root = source_read_support::workspace_root(lease)?;
    // 缺省路径只经内部 root helper 取得 root；显式路径仍必须经过唯一 Resolver。
    let target = match relative_path {
        Some(relative_path) => source_read_support::resolve_relative_path(lease, relative_path)?,
        None => root.clone(),
    };
    let metadata = fs::metadata(&target).map_err(|_| "INVALID_PATH: target is unavailable")?;
    if !metadata.is_dir() {
        return Err("INVALID_PATH: expected a directory".into());
    }
    let matcher = file_mask_matcher(file_mask)?;
    let mut matches = FileMatches::default();
    if serialized_matches(&matches).len() > max_bytes {
        return finish_matches(matches, true);
    }

    let builder = workspace_walk_builder(&root, &target, lease, limits.max_depth)?;
    let started = Instant::now();
    let mut entries_seen = 0usize;
    let mut truncated = false;
    for entry in builder.build() {
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

        let entry = entry.map_err(|_| "INVALID_PATH: directory entry could not be inspected")?;
        // max_depth + 1 仅作为探针；实际遇到下一层时停止，避免悄然漏报 truncated。
        if entry.depth() > limits.max_depth {
            truncated = true;
            break;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| "INVALID_PATH: directory entry could not be inspected")?;
        if metadata.is_dir() || is_reparse_point(&metadata) {
            continue;
        }
        if !metadata.is_file() || !matcher.is_match(Path::new(entry.file_name())) {
            continue;
        }
        if matches.files.len() >= limits.max_matches {
            truncated = true;
            break;
        }
        let relative = workspace_relative_path(&root, path)?;
        if !try_add_match(&mut matches, relative, max_bytes) {
            truncated = true;
            break;
        }
    }
    finish_matches(matches, truncated)
}

/// 创建只读取 Workspace 内 `.gitignore` 的 walker，关闭机器级与祖先隐式规则。
fn workspace_walk_builder(
    root: &Path,
    target: &Path,
    lease: &WorkspaceLease,
    max_depth: usize,
) -> Result<WalkBuilder, String> {
    let mut builder = WalkBuilder::new(target);
    builder
        .current_dir(root)
        .follow_links(false)
        .hidden(true)
        .parents(false)
        .ignore(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .max_depth(Some(max_depth.saturating_add(1)));
    add_workspace_ancestor_gitignores(&mut builder, root, target)?;

    // filter_entry 在 descend 前执行；每个目录都以同一 Lease 再验证，链接/reparse 一律不入树。
    let filter_root = root.to_path_buf();
    let filter_lease = Arc::new(lease.clone());
    builder.filter_entry(move |entry| {
        if entry.depth() == 0 {
            return true;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return false;
        };
        if is_reparse_point(&metadata) {
            return false;
        }
        if !metadata.is_dir() {
            return true;
        }
        let Ok(relative) = workspace_relative_path(&filter_root, path) else {
            return false;
        };
        source_read_support::resolve_relative_path(&filter_lease, &relative).is_ok()
    });
    Ok(builder)
}

/// 仅加入 root 到 target 之间的 `.gitignore`，绝不向 Workspace 外父目录探测。
fn add_workspace_ancestor_gitignores(
    builder: &mut WalkBuilder,
    root: &Path,
    target: &Path,
) -> Result<(), String> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| "INVALID_PATH: path escapes the workspace root")?;
    let mut current = root.to_path_buf();
    add_gitignore_if_present(builder, &current)?;
    for component in relative.components() {
        current.push(component);
        add_gitignore_if_present(builder, &current)?;
    }
    Ok(())
}

/// 只把存在的 Workspace-local `.gitignore` 加入 walker，解析错误必须显式失败而不是静默改变结果。
fn add_gitignore_if_present(builder: &mut WalkBuilder, directory: &Path) -> Result<(), String> {
    let ignore_file = directory.join(".gitignore");
    match fs::symlink_metadata(&ignore_file) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("INVALID_PATH: .gitignore could not be inspected".into()),
        Ok(metadata) if is_reparse_point(&metadata) || !metadata.is_file() => {
            Err("INVALID_PATH: .gitignore must be a regular workspace file".into())
        }
        Ok(_) if builder.add_ignore(ignore_file).is_some() => {
            Err("INVALID_PATH: .gitignore could not be read".into())
        }
        Ok(_) => Ok(()),
    }
}

/// 构建 basename glob；Windows compatibility 明确要求 case-insensitive，其他平台保留原语义。
fn file_mask_matcher(file_mask: &str) -> Result<GlobMatcher, String> {
    let mut builder = GlobBuilder::new(file_mask);
    #[cfg(windows)]
    builder.case_insensitive(true);
    builder
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|_| "INVALID_PARAMS: invalid file_mask".into())
}

/// 在每个 iterator entry 前统一检查 cancellation 与内部 deadline。
fn should_stop(
    cancel: &CancellationToken,
    started: Instant,
    timeout: Duration,
) -> Result<bool, String> {
    source_read_support::check_cancelled(cancel)?;
    Ok(started.elapsed() >= timeout)
}

/// 只以完整 JSON 的序列化长度判断 public budget，避免输出半个 UTF-8 或 JSON token。
fn try_add_match(matches: &mut FileMatches, path: String, max_bytes: usize) -> bool {
    if !matches.files.insert(path.clone()) {
        return true;
    }
    if serialized_matches(matches).len() <= max_bytes {
        return true;
    }
    matches.files.remove(&path);
    false
}

/// 序列化是唯一的输出长度度量，保证任何提前停止都保留合法 JSON object。
fn serialized_matches(matches: &FileMatches) -> String {
    serde_json::to_string(matches).expect("File matches must serialize")
}

/// 收尾始终生成完整 JSON，避免按 byte slice 截断 UTF-8 或 JSON token。
fn finish_matches(matches: FileMatches, truncated: bool) -> Result<FindResult, String> {
    Ok(FindResult {
        text: serialized_matches(&matches),
        truncated,
    })
}

/// 将已经由 Lease root 约束的路径投影成当前平台兼容的 Workspace-relative 文本。
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

/// 检测链接与 Windows reparse point，二者都不得进入递归或作为查找结果。
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
#[path = "source_find_tests.rs"]
mod tests;
