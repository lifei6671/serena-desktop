use super::{registry::SourceArgs, source_read_support};
use crate::workspace_resolver::WorkspaceLease;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path, time::SystemTime};
use tokio_util::sync::CancellationToken;

const DEFAULT_MAX_BYTES: usize = 32_768;
const MAX_MAX_BYTES: usize = 131_072;
const READ_CHUNK_BYTES: usize = 64 * 1024;

/// 同一路径的完整文件版本；只有 before/after 一致时才能产生公开结果。
#[derive(Debug, PartialEq, Eq)]
struct Version {
    path: String,
    sha256: String,
    modified: SystemTime,
    len: u64,
}

/// 本地读取完成后保留的、不会泄露绝对根目录的文件版本与正文。
struct ReadResult {
    version: Version,
    text: String,
    truncated: bool,
}

/// 从已捕获 Lease 的 Workspace 中读取一个文本文件；不调用 Serena 或 Capability Manager。
pub(super) async fn read(
    lease: &WorkspaceLease,
    arguments: SourceArgs,
    cancel: CancellationToken,
) -> Result<Value, String> {
    let relative_path = arguments
        .relative_path
        .ok_or("INVALID_PARAMS: 缺少 relative_path")?;
    let max_bytes = max_bytes(arguments.max_bytes)?;
    let preserve_terminal_newline = arguments.start_line.is_none() && arguments.end_line.is_none();
    let start_line = arguments.start_line.unwrap_or(0);
    let end_line = arguments.end_line;
    let lease = lease.clone();
    let response_lease = lease.clone();

    let result = source_read_support::run_blocking_cancellable(cancel, move |worker_cancel| {
        read_and_recapture(
            &lease,
            &relative_path,
            start_line,
            end_line,
            preserve_terminal_newline,
            max_bytes,
            &worker_cancel,
        )
    })
    .await?;

    let mut response =
        serde_json::to_value(source_read_support::workspace_provenance(&response_lease))
            .expect("Workspace provenance must serialize");
    let ReadResult {
        version,
        text,
        truncated,
    } = result;
    response["text"] = json!(text);
    response["truncated"] = json!(truncated);
    response["path"] = json!(version.path);
    response["sha256"] = json!(version.sha256);
    Ok(response)
}

/// 验证公共 `max_bytes` 预算，客户端不得把本地读取放大到 hard max 以上。
fn max_bytes(value: Option<usize>) -> Result<usize, String> {
    let value = value.unwrap_or(DEFAULT_MAX_BYTES);
    if value == 0 || value > MAX_MAX_BYTES {
        return Err("INVALID_PARAMS: max_bytes 超出范围".into());
    }
    Ok(value)
}

/// 读取后再次按同一 Lease 与相对路径捕获当前版本，拒绝中途替换、重定向或内容变化。
fn read_and_recapture(
    lease: &WorkspaceLease,
    relative_path: &str,
    start_line: u32,
    end_line: Option<u32>,
    preserve_terminal_newline: bool,
    max_bytes: usize,
    cancel: &CancellationToken,
) -> Result<ReadResult, String> {
    read_and_recapture_with_hooks(
        lease,
        relative_path,
        start_line,
        end_line,
        preserve_terminal_newline,
        max_bytes,
        cancel,
        |_| {},
        |_| {},
    )
}

/// 执行 before/after 一致性检查；两个测试 hook 精确控制正文读取与 post-capture 的并发时序。
#[expect(
    clippy::too_many_arguments,
    reason = "测试 seam 需显式保留读取参数、取消 token 与两个并发时序 hook，避免改变版本捕获语义"
)]
fn read_and_recapture_with_hooks<ReadHook, CaptureHook>(
    lease: &WorkspaceLease,
    relative_path: &str,
    start_line: u32,
    end_line: Option<u32>,
    preserve_terminal_newline: bool,
    max_bytes: usize,
    cancel: &CancellationToken,
    on_read_chunk: ReadHook,
    on_capture_chunk: CaptureHook,
) -> Result<ReadResult, String>
where
    ReadHook: FnMut(usize),
    CaptureHook: FnMut(usize),
{
    let before = read_file_with_chunk_hook(
        lease,
        relative_path,
        start_line,
        end_line,
        preserve_terminal_newline,
        max_bytes,
        cancel,
        on_read_chunk,
    )?;
    let after =
        capture_current_version_with_chunk_hook(lease, relative_path, cancel, on_capture_chunk)
            .map_err(post_read_consistency_error)?;
    if before.version != after {
        return Err("SOURCE_READ_CHANGED".into());
    }
    Ok(before)
}

/// 执行实际正文读取；测试 hook 只用于证明取消或替换发生在已打开旧句柄之后。
fn read_file_with_chunk_hook<Hook>(
    lease: &WorkspaceLease,
    relative_path: &str,
    start_line: u32,
    end_line: Option<u32>,
    preserve_terminal_newline: bool,
    max_bytes: usize,
    cancel: &CancellationToken,
    mut on_chunk: Hook,
) -> Result<ReadResult, String>
where
    Hook: FnMut(usize),
{
    source_read_support::check_cancelled(cancel)?;
    let root = source_read_support::workspace_root(lease)?;
    let resolved = source_read_support::resolve_relative_path(lease, relative_path)?;
    let metadata = fs::metadata(&resolved).map_err(|_| "INVALID_PATH: target is unavailable")?;
    if !metadata.is_file() {
        return Err("INVALID_PATH: expected a regular file".into());
    }
    let modified = metadata
        .modified()
        .map_err(|_| "INVALID_PATH: target metadata is unavailable")?;
    let initial_len = metadata.len();
    let mut file = fs::File::open(&resolved).map_err(|_| "INVALID_PATH: target is unavailable")?;
    let mut remaining = initial_len;
    let mut hash = Sha256::new();
    let mut collector =
        TextCollector::new(start_line, end_line, preserve_terminal_newline, max_bytes);
    let mut utf8_tail = Vec::new();
    let mut chunk = [0; READ_CHUNK_BYTES];

    while remaining > 0 {
        source_read_support::check_cancelled(cancel)?;
        let read_size = remaining.min(chunk.len() as u64) as usize;
        let read = file
            .read(&mut chunk[..read_size])
            .map_err(|_| "INVALID_PATH: target could not be read")?;
        if read == 0 {
            return Err("SOURCE_READ_CHANGED".into());
        }
        let bytes = &chunk[..read];
        if bytes.contains(&0) {
            // 复用既有后端错误类别，不向公开 taxonomy 增加新的 binary 专用错误码。
            return Err("BACKEND_ERROR: source_read_file only supports UTF-8 text".into());
        }
        hash.update(bytes);
        on_chunk(read);
        source_read_support::check_cancelled(cancel)?;
        consume_utf8(&mut utf8_tail, bytes, &mut collector, cancel)?;
        remaining -= read as u64;
    }
    if !utf8_tail.is_empty() {
        return Err("BACKEND_ERROR: source_read_file only supports UTF-8 text".into());
    }
    collector.finish();

    // 读取只覆盖打开句柄的初始长度；路径当前指向的版本由 post-capture 另行校验。
    let after = file
        .metadata()
        .map_err(|_| "INVALID_PATH: target metadata is unavailable")?;
    if after.len() != initial_len
        || after
            .modified()
            .map_err(|_| "INVALID_PATH: target metadata is unavailable")?
            != modified
    {
        return Err("SOURCE_READ_CHANGED".into());
    }

    Ok(ReadResult {
        version: Version {
            path: workspace_relative_path(&root, &resolved)?,
            sha256: hex_digest(hash),
            modified,
            len: initial_len,
        },
        text: collector.text,
        truncated: collector.truncated,
    })
}

/// 按同一 captured Lease 重新解析并流式哈希当前路径版本，绝不复用之前打开的 File handle。
fn capture_current_version_with_chunk_hook<Hook>(
    lease: &WorkspaceLease,
    relative_path: &str,
    cancel: &CancellationToken,
    mut on_chunk: Hook,
) -> Result<Version, String>
where
    Hook: FnMut(usize),
{
    source_read_support::check_cancelled(cancel)?;
    let root = source_read_support::workspace_root(lease)?;
    let resolved = source_read_support::resolve_relative_path(lease, relative_path)?;
    let metadata = fs::metadata(&resolved).map_err(|_| "INVALID_PATH: target is unavailable")?;
    if !metadata.is_file() {
        return Err("INVALID_PATH: expected a regular file".into());
    }
    let modified = metadata
        .modified()
        .map_err(|_| "INVALID_PATH: target metadata is unavailable")?;
    let len = metadata.len();
    let mut file = fs::File::open(&resolved).map_err(|_| "INVALID_PATH: target is unavailable")?;
    let mut remaining = len;
    let mut hash = Sha256::new();
    let mut chunk = [0; READ_CHUNK_BYTES];

    while remaining > 0 {
        source_read_support::check_cancelled(cancel)?;
        let read_size = remaining.min(chunk.len() as u64) as usize;
        let read = file
            .read(&mut chunk[..read_size])
            .map_err(|_| "INVALID_PATH: target could not be read")?;
        if read == 0 {
            return Err("SOURCE_READ_CHANGED".into());
        }
        hash.update(&chunk[..read]);
        on_chunk(read);
        source_read_support::check_cancelled(cancel)?;
        remaining -= read as u64;
    }
    let after = file
        .metadata()
        .map_err(|_| "INVALID_PATH: target metadata is unavailable")?;
    if after.len() != len
        || after
            .modified()
            .map_err(|_| "INVALID_PATH: target metadata is unavailable")?
            != modified
    {
        return Err("SOURCE_READ_CHANGED".into());
    }

    Ok(Version {
        path: workspace_relative_path(&root, &resolved)?,
        sha256: hex_digest(hash),
        modified,
        len,
    })
}

/// post-capture 的任何非取消失败都表示读取期间路径或版本不再与正文读取时一致。
fn post_read_consistency_error(error: String) -> String {
    if error == "CANCELLED" {
        error
    } else {
        "SOURCE_READ_CHANGED".into()
    }
}

/// 把完成的 SHA-256 转为冻结 wire contract 所需的小写十六进制字符串。
fn hex_digest(hash: Sha256) -> String {
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 将已解析的路径投影成跨平台稳定的 Workspace-relative slash 路径。
fn workspace_relative_path(root: &Path, resolved: &Path) -> Result<String, String> {
    resolved
        .strip_prefix(root)
        .map_err(|_| "INVALID_PATH: path escapes the workspace root")?
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .ok_or_else(|| "INVALID_PATH: path is not valid Unicode".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|components| components.join("/"))
}

/// 将分块字节严格解码为 UTF-8，最多仅保留三个跨块残留字节。
fn consume_utf8(
    tail: &mut Vec<u8>,
    bytes: &[u8],
    collector: &mut TextCollector,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let mut combined = Vec::with_capacity(tail.len() + bytes.len());
    combined.extend_from_slice(tail);
    combined.extend_from_slice(bytes);
    match std::str::from_utf8(&combined) {
        Ok(text) => {
            collector.push(text, cancel)?;
            tail.clear();
            Ok(())
        }
        Err(error) => {
            let valid = error.valid_up_to();
            let prefix = std::str::from_utf8(&combined[..valid])
                .map_err(|_| "BACKEND_ERROR: source_read_file only supports UTF-8 text")?;
            collector.push(prefix, cancel)?;
            if error.error_len().is_some() {
                return Err("BACKEND_ERROR: source_read_file only supports UTF-8 text".into());
            }
            tail.clear();
            tail.extend_from_slice(&combined[valid..]);
            Ok(())
        }
    }
}

/// 以 Serena 既有的零基、包含两端的 line range 语义收集正文，同时保持 UTF-8 预算边界。
struct TextCollector {
    text: String,
    truncated: bool,
    start_line: u32,
    end_line: Option<u32>,
    max_bytes: usize,
    line_index: u64,
    line_started: bool,
    emitted_line: bool,
    line_has_content: bool,
    pending_cr: bool,
    preserve_terminal_newline: bool,
    ended_with_line_terminator: bool,
}

impl TextCollector {
    /// 创建仅收集目标行范围、且正文永不超过 byte budget 的流式收集器。
    fn new(
        start_line: u32,
        end_line: Option<u32>,
        preserve_terminal_newline: bool,
        max_bytes: usize,
    ) -> Self {
        Self {
            text: String::new(),
            truncated: false,
            start_line,
            end_line,
            max_bytes,
            line_index: 0,
            line_started: false,
            emitted_line: false,
            line_has_content: false,
            pending_cr: false,
            preserve_terminal_newline,
            ended_with_line_terminator: false,
        }
    }

    /// 逐字符处理严格 UTF-8 正文，避免跨 chunk、CRLF 与 UTF-8 边界改变公开输出。
    fn push(&mut self, text: &str, cancel: &CancellationToken) -> Result<(), String> {
        for character in text.chars() {
            source_read_support::check_cancelled(cancel)?;
            if self.pending_cr {
                self.pending_cr = false;
                if character == '\n' {
                    self.finish_line();
                    continue;
                }
                self.append_character('\r');
            }
            match character {
                '\r' => {
                    self.line_has_content = true;
                    self.pending_cr = true;
                    self.ended_with_line_terminator = false;
                }
                '\n' => self.finish_line(),
                character => {
                    self.line_has_content = true;
                    self.ended_with_line_terminator = false;
                    self.append_character(character);
                }
            }
        }
        Ok(())
    }

    /// 刷新末尾的 lone CR；完整读取保留真实终止分隔符，行范围继续采用既有连接语义。
    fn finish(&mut self) {
        if self.pending_cr {
            self.pending_cr = false;
            self.append_character('\r');
        }
        if self.line_has_content && self.selected_line() {
            self.begin_line();
        }
        if self.preserve_terminal_newline && self.ended_with_line_terminator {
            self.append('\n');
        }
    }

    /// 结束当前行：空行也属于 `.lines()` 返回的行，因此必须参与选择与连接。
    fn finish_line(&mut self) {
        if self.selected_line() {
            self.begin_line();
        }
        self.line_index += 1;
        self.line_started = false;
        self.line_has_content = false;
        self.ended_with_line_terminator = true;
    }

    /// 启动一个选择到的行，并只在相邻的已选择行之间写入规范换行符。
    fn begin_line(&mut self) {
        if self.line_started {
            return;
        }
        if self.emitted_line {
            self.append('\n');
        }
        self.emitted_line = true;
        self.line_started = true;
    }

    /// 追加当前行中的单个字符，预算不足时只标记截断而不破坏 UTF-8。
    fn append_character(&mut self, character: char) {
        if !self.selected_line() {
            return;
        }
        self.begin_line();
        self.append(character);
    }

    /// 追加一个完整 UTF-8 字符；从不按中间字节切割 code point。
    fn append(&mut self, character: char) {
        if self.truncated {
            return;
        }
        if self.text.len() + character.len_utf8() > self.max_bytes {
            self.truncated = true;
            return;
        }
        self.text.push(character);
    }

    /// 判断当前零基行是否位于包含两端的请求范围中。
    fn selected_line(&self) -> bool {
        self.line_index >= u64::from(self.start_line)
            && self
                .end_line
                .is_none_or(|end_line| self.line_index <= u64::from(end_line))
    }
}

#[cfg(test)]
#[path = "source_read_tests.rs"]
mod tests;
