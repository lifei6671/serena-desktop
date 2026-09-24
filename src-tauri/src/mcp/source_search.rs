use super::{registry::SourceArgs, source_read_support};
use crate::workspace_resolver::WorkspaceLease;
use ignore::WalkBuilder;
use regex::Regex;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufReader, Read},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const DEFAULT_MAX_BYTES: usize = 65_536;
const MAX_MAX_BYTES: usize = 262_144;
// 搜索只读取受控大小的普通文件；超过此上限代表结果不完整而非公开参数。
const MAX_FILE_BYTES: u64 = 1_048_576;
const MAX_ENTRIES: usize = 100_000;
const MAX_MATCHES: usize = 10_000;
const MAX_DEPTH: usize = 64;
const SEARCH_TIMEOUT: Duration = Duration::from_secs(5);
const READ_CHUNK_BYTES: usize = 8_192;

/// 内部搜索硬界集中定义，测试可用极小值覆盖每个停止分支。
#[derive(Clone, Copy)]
struct SearchLimits {
    max_file_bytes: u64,
    max_entries: usize,
    max_matches: usize,
    max_depth: usize,
    timeout: Duration,
}

impl SearchLimits {
    /// 返回 production 中不影响普通源码树的防病态硬界。
    fn production() -> Self {
        Self {
            max_file_bytes: MAX_FILE_BYTES,
            max_entries: MAX_ENTRIES,
            max_matches: MAX_MATCHES,
            max_depth: MAX_DEPTH,
            timeout: SEARCH_TIMEOUT,
        }
    }
}

/// 本地 `source_search_pattern` 只使用 captured Lease，不调用 Serena 或 Capability Manager。
pub(super) async fn search(
    lease: &WorkspaceLease,
    arguments: SourceArgs,
    cancel: CancellationToken,
) -> Result<Value, String> {
    let substring_pattern = arguments
        .substring_pattern
        .ok_or("INVALID_PARAMS: 缺少 substring_pattern")?;
    let max_bytes = max_bytes(arguments.max_bytes)?;
    let lease = lease.clone();
    let response_lease = lease.clone();

    let result = source_read_support::run_blocking_cancellable(cancel, move |worker_cancel| {
        search_with_limits(
            &lease,
            arguments.relative_path.as_deref(),
            &substring_pattern,
            max_bytes,
            &worker_cancel,
            SearchLimits::production(),
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

/// 继承 compatibility Source search 的公开 byte budget。
fn max_bytes(value: Option<usize>) -> Result<usize, String> {
    let value = value.unwrap_or(DEFAULT_MAX_BYTES);
    if value == 0 || value > MAX_MAX_BYTES {
        return Err("INVALID_PARAMS: max_bytes 超出范围".into());
    }
    Ok(value)
}

/// 完整 JSON 文本与是否因公开预算或内部硬界提前停止。
struct SearchResult {
    text: String,
    truncated: bool,
}

/// 同时返回第一遍流式验证的明确状态，确保后置 binary 标记不会泄露任何早期命中。
enum TextFileStatus {
    Valid,
    Binary,
    TooLarge,
    Stopped,
}

/// 生产入口不需要测试 hook；所有 I/O 循环仍复用同一个 CancellationToken。
fn search_with_limits(
    lease: &WorkspaceLease,
    relative_path: Option<&str>,
    substring_pattern: &str,
    max_bytes: usize,
    cancel: &CancellationToken,
    limits: SearchLimits,
) -> Result<SearchResult, String> {
    search_with_limits_and_hooks(
        lease,
        relative_path,
        substring_pattern,
        max_bytes,
        cancel,
        limits,
        |_| {},
        || {},
    )
}

/// 测试 hook 精确控制 traversal 与文件读取取消时序；production 均为无操作。
#[expect(
    clippy::too_many_arguments,
    reason = "测试 seam 需显式保留搜索预算、取消 token 与 traversal/读取 hook，避免改变预算边界"
)]
fn search_with_limits_and_hooks<EntryHook, ReadHook>(
    lease: &WorkspaceLease,
    relative_path: Option<&str>,
    substring_pattern: &str,
    max_bytes: usize,
    cancel: &CancellationToken,
    limits: SearchLimits,
    mut on_entry: EntryHook,
    mut on_read: ReadHook,
) -> Result<SearchResult, String>
where
    EntryHook: FnMut(usize),
    ReadHook: FnMut(),
{
    source_read_support::check_cancelled(cancel)?;
    let root = source_read_support::workspace_root(lease)?;
    let target = match relative_path {
        Some(relative_path) => source_read_support::resolve_relative_path(lease, relative_path)?,
        None => root.clone(),
    };
    let metadata = fs::metadata(&target).map_err(|_| "INVALID_PATH: target is unavailable")?;
    if !metadata.is_dir() && !metadata.is_file() {
        return Err("INVALID_PATH: expected a file or directory".into());
    }
    // Host compatibility：非法 regex 是成功的空结果，而不是公开参数错误。
    let Ok(regex) = Regex::new(substring_pattern) else {
        return finish_matches(BTreeMap::new(), false);
    };

    let mut matches = BTreeMap::<String, Vec<String>>::new();
    if serialized_matches(&matches).len() > max_bytes {
        return finish_matches(matches, true);
    }
    let started = Instant::now();
    let mut match_count = 0usize;

    // 显式文件路径直接搜索该文件，避免调用方为了单文件搜索先退化为全目录扫描。
    if metadata.is_file() {
        if metadata.len() > limits.max_file_bytes {
            return finish_matches(matches, true);
        }
        match validate_text_file(&target, cancel, started, limits, &mut on_read)? {
            TextFileStatus::Valid => {}
            TextFileStatus::Binary => return finish_matches(matches, false),
            TextFileStatus::TooLarge | TextFileStatus::Stopped => {
                return finish_matches(matches, true);
            }
        }
        let relative = workspace_relative_path(&root, &target)?;
        let truncated = search_text_file(
            &target,
            &relative,
            &regex,
            max_bytes,
            &mut matches,
            &mut match_count,
            cancel,
            started,
            limits,
            &mut on_read,
        )?;
        return finish_matches(matches, truncated);
    }

    let builder = workspace_walk_builder(&root, &target, lease, limits.max_depth)?;
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
        if entry.depth() > limits.max_depth {
            truncated = true;
            break;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| "INVALID_PATH: directory entry could not be inspected")?;
        if metadata.is_dir() || is_reparse_point(&metadata) || !metadata.is_file() {
            continue;
        }
        if metadata.len() > limits.max_file_bytes {
            truncated = true;
            continue;
        }

        match validate_text_file(path, cancel, started, limits, &mut on_read)? {
            TextFileStatus::Valid => {}
            // NUL 与非法 UTF-8 均跳过整个文件，不是错误或截断。
            TextFileStatus::Binary => continue,
            TextFileStatus::TooLarge | TextFileStatus::Stopped => {
                truncated = true;
                if matches!(validate_stop(cancel, started, limits.timeout)?, Some(())) {
                    break;
                }
                continue;
            }
        }

        let relative = workspace_relative_path(&root, path)?;
        let file_result = search_text_file(
            path,
            &relative,
            &regex,
            max_bytes,
            &mut matches,
            &mut match_count,
            cancel,
            started,
            limits,
            &mut on_read,
        )?;
        if file_result {
            truncated = true;
            break;
        }
    }
    finish_matches(matches, truncated)
}

/// 第一遍只验证有界文件的全部字节；遇到后置 binary 标记也不会让第二遍开始。
fn validate_text_file<ReadHook>(
    path: &Path,
    cancel: &CancellationToken,
    started: Instant,
    limits: SearchLimits,
    on_read: &mut ReadHook,
) -> Result<TextFileStatus, String>
where
    ReadHook: FnMut(),
{
    let file = fs::File::open(path).map_err(|_| "INVALID_PATH: file could not be read")?;
    let mut reader = BufReader::new(file);
    let mut buffer = [0u8; READ_CHUNK_BYTES];
    let mut pending = Vec::new();
    let mut bytes_read = 0u64;
    loop {
        if validate_stop(cancel, started, limits.timeout)?.is_some() {
            return Ok(TextFileStatus::Stopped);
        }
        on_read();
        let read = reader
            .read(&mut buffer)
            .map_err(|_| "INVALID_PATH: file could not be read")?;
        if read == 0 {
            return Ok(if pending.is_empty() {
                TextFileStatus::Valid
            } else {
                TextFileStatus::Binary
            });
        }
        bytes_read = bytes_read.saturating_add(read as u64);
        if bytes_read > limits.max_file_bytes {
            return Ok(TextFileStatus::TooLarge);
        }
        let chunk = &buffer[..read];
        if chunk.contains(&0) {
            return Ok(TextFileStatus::Binary);
        }
        pending.extend_from_slice(chunk);
        match std::str::from_utf8(&pending) {
            Ok(_) => pending.clear(),
            Err(error) if error.error_len().is_some() => return Ok(TextFileStatus::Binary),
            Err(error) => {
                let valid = error.valid_up_to();
                pending.drain(..valid);
            }
        }
    }
}

/// 第二遍才逐行执行已编译 regex；每条候选结果都先以最终 JSON byte 长度提交。
#[allow(clippy::too_many_arguments)]
fn search_text_file<ReadHook>(
    path: &Path,
    relative: &str,
    regex: &Regex,
    max_bytes: usize,
    matches: &mut BTreeMap<String, Vec<String>>,
    match_count: &mut usize,
    cancel: &CancellationToken,
    started: Instant,
    limits: SearchLimits,
    on_read: &mut ReadHook,
) -> Result<bool, String>
where
    ReadHook: FnMut(),
{
    let file = fs::File::open(path).map_err(|_| "INVALID_PATH: file could not be read")?;
    let mut reader = BufReader::new(file);
    let mut buffer = [0u8; READ_CHUNK_BYTES];
    let mut line = Vec::new();
    let mut line_number = 0usize;
    let mut bytes_read = 0u64;
    loop {
        if validate_stop(cancel, started, limits.timeout)?.is_some() {
            return Ok(true);
        }
        on_read();
        let read = reader
            .read(&mut buffer)
            .map_err(|_| "INVALID_PATH: file could not be read")?;
        if read == 0 {
            if line.is_empty() {
                return Ok(false);
            }
            return submit_line(
                &mut line,
                line_number,
                relative,
                regex,
                max_bytes,
                matches,
                match_count,
                limits.max_matches,
            );
        }
        bytes_read = bytes_read.saturating_add(read as u64);
        if bytes_read > limits.max_file_bytes {
            return Ok(true);
        }
        for &byte in &buffer[..read] {
            if byte == b'\n' {
                if validate_stop(cancel, started, limits.timeout)?.is_some() {
                    return Ok(true);
                }
                if submit_line(
                    &mut line,
                    line_number,
                    relative,
                    regex,
                    max_bytes,
                    matches,
                    match_count,
                    limits.max_matches,
                )? {
                    return Ok(true);
                }
                line_number = line_number.saturating_add(1);
            } else {
                line.push(byte);
            }
        }
    }
}

/// 只在完整行命中后提交；超出预算或 match hard limit 时撤回并通知上层停止。
#[expect(
    clippy::too_many_arguments,
    reason = "流式搜索必须在单行提交点聚合 CRLF、regex、JSON byte budget 与 match limit 状态"
)]
fn submit_line(
    line: &mut Vec<u8>,
    line_number: usize,
    relative: &str,
    regex: &Regex,
    max_bytes: usize,
    matches: &mut BTreeMap<String, Vec<String>>,
    match_count: &mut usize,
    max_matches: usize,
) -> Result<bool, String> {
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    let text = std::str::from_utf8(line).map_err(|_| "INVALID_PATH: validated file changed")?;
    let should_add = regex.is_match(text);
    if should_add && *match_count >= max_matches {
        return Ok(true);
    }
    if should_add {
        let record = format!("  >{:4}:{text}", line_number);
        matches.entry(relative.to_owned()).or_default().push(record);
        if serialized_matches(matches).len() > max_bytes {
            let empty = {
                let values = matches
                    .get_mut(relative)
                    .expect("match entry was just inserted");
                values.pop();
                values.is_empty()
            };
            if empty {
                matches.remove(relative);
            }
            return Ok(true);
        }
        *match_count += 1;
    }
    line.clear();
    Ok(false)
}

/// 所有循环以相同的 token 和 deadline 检查；取消仍保持公开 CANCELLED 错误。
fn validate_stop(
    cancel: &CancellationToken,
    started: Instant,
    timeout: Duration,
) -> Result<Option<()>, String> {
    source_read_support::check_cancelled(cancel)?;
    Ok((started.elapsed() >= timeout).then_some(()))
}

/// walker entry 前的同一取消和 deadline 检查。
fn should_stop(
    cancel: &CancellationToken,
    started: Instant,
    timeout: Duration,
) -> Result<bool, String> {
    Ok(validate_stop(cancel, started, timeout)?.is_some())
}

/// 与 P2B-004 相同地仅启用 Workspace-local `.gitignore`，并对默认 traversal 跳过 hidden。
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
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .max_depth(Some(max_depth.saturating_add(1)));
    add_workspace_ancestor_gitignores(&mut builder, root, target)?;

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
        if is_reparse_point(&metadata) || is_hidden_name(entry.file_name()) {
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

/// 隐藏判断只作用于 target 之下的 entry，因此显式 `.serena` target 仍允许搜索。
fn is_hidden_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

/// 仅加入 root 到显式 subtree 间的 `.gitignore`，绝不读取 Workspace 外祖先规则。
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

/// `.gitignore` 自身也必须是普通 Workspace 文件，错误不静默降级为其他 ignore 语义。
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

/// 仅返回 Workspace-relative 的当前平台分隔符文本，绝不泄露 canonical root。
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

/// 链接与 Windows reparse point 都不得作为文件结果或递归进入。
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

/// 最终长度唯一以 serde JSON bytes 衡量，任何停止均保留完整 JSON object。
fn serialized_matches(matches: &BTreeMap<String, Vec<String>>) -> String {
    serde_json::to_string(matches).expect("Search matches must serialize")
}

/// 收尾返回稳定 JSON map，不保留无 match 的空 key。
fn finish_matches(
    matches: BTreeMap<String, Vec<String>>,
    truncated: bool,
) -> Result<SearchResult, String> {
    Ok(SearchResult {
        text: serialized_matches(&matches),
        truncated,
    })
}

#[cfg(test)]
#[path = "source_search_tests.rs"]
mod tests;
