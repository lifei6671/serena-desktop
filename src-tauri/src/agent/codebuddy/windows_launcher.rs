//! CodeBuddy Windows Job-at-creation launcher；不包含 ACP、Runtime、持久化或恢复逻辑。

use std::{
    cmp::Ordering,
    ffi::{OsStr, OsString, c_void},
    fmt,
    fs::{self, File},
    mem::{size_of, zeroed},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Foundation::*,
    Globalization::{CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN, CompareStringOrdinal},
    System::{JobObjects::*, Pipes::CreatePipe, Threading::*},
};

use super::discovery::ResolvedLaunchSpec;
use crate::config::{canonicalize_workspace_root, same_workspace_root_identity};

const INVALID_INPUT: &str = "CODEBUDDY_LAUNCH_INPUT_INVALID";
const INVALID_WORKSPACE_PATH: &str = "CODEBUDDY_EXTERNAL_WORKSPACE_PATH_INVALID";
const INVALID_SCRIPT_PATH: &str = "CODEBUDDY_EXTERNAL_SCRIPT_PATH_INVALID";
const UNSUPPORTED_UNC_CWD: &str = "CODEBUDDY_UNC_CWD_UNSUPPORTED";

/// 声明 launcher/provider 是否明确支持普通 UNC current directory。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UncCurrentDirectoryPolicy {
    Supported,
    Unsupported,
}

/// 已按 Workspace identity authority 验证的单一外部进程路径投影。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ExternalProcessPath {
    projected: PathBuf,
}

impl ExternalProcessPath {
    /// 投影 frozen canonical root，并确认投影仍指向同一 Workspace。
    pub(crate) fn verify(
        frozen_canonical_root: &Path,
        unc_policy: UncCurrentDirectoryPolicy,
    ) -> Result<Self, LaunchError> {
        let projected = project_external_path(frozen_canonical_root, unc_policy)?;
        verify_projected_identity(frozen_canonical_root, &projected)?;
        Ok(Self { projected })
    }

    /// 返回供 `CreateProcessW.current_dir` 与后续 `session/new.cwd` 共用的结果。
    pub(crate) fn as_path(&self) -> &Path {
        &self.projected
    }
}

/// 通过现有 config authority 验证 external projection 的 Workspace identity。
fn verify_projected_identity(
    frozen_canonical_root: &Path,
    projected: &Path,
) -> Result<(), LaunchError> {
    let projected_canonical = canonicalize_workspace_root(projected)
        .map_err(|_| failure(INVALID_WORKSPACE_PATH, ERROR_BAD_PATHNAME))?;
    if !same_workspace_root_identity(frozen_canonical_root, &projected_canonical) {
        return Err(failure(INVALID_WORKSPACE_PATH, ERROR_BAD_PATHNAME));
    }
    Ok(())
}

impl fmt::Debug for ExternalProcessPath {
    /// 仅展示外部边界路径；该值不具备数据库 Workspace authority。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ExternalProcessPath")
            .field(&self.projected)
            .finish()
    }
}

/// 本次 Provider 子进程的完整、私有 Unicode environment block。
pub(crate) struct ProviderChildEnvironment {
    block: Vec<u16>,
    variable_count: usize,
    path_entry_count: usize,
}

impl ProviderChildEnvironment {
    /// 复制当前 Host environment，并仅用 resolver PATH projection 覆盖 PATH。
    pub(crate) fn from_host(path_projection: &[PathBuf]) -> Result<Self, LaunchError> {
        Self::from_entries(std::env::vars_os().collect(), path_projection)
    }

    /// 从可注入基线构造 block；生产路径只通过 `from_host` 调用。
    fn from_entries(
        mut entries: Vec<(OsString, OsString)>,
        path_projection: &[PathBuf],
    ) -> Result<Self, LaunchError> {
        // Windows 环境变量名大小写不敏感，必须移除所有 PATH 拼写后只插入一个覆盖值。
        entries.retain(|(name, _)| !os_eq_ignore_case(name, OsStr::new("PATH")));
        let path = std::env::join_paths(path_projection)
            .map_err(|_| failure(INVALID_INPUT, ERROR_INVALID_PARAMETER))?;
        entries.push((OsString::from("Path"), path));

        for (name, value) in &entries {
            validate_environment_entry(name, value)?;
        }
        entries.sort_by(|(left, _), (right, _)| compare_os_ignore_case(left, right));

        let mut block = Vec::new();
        for (name, value) in &entries {
            block.extend(name.encode_wide());
            block.push(u16::from(b'='));
            block.extend(value.encode_wide());
            block.push(0);
        }
        // CreateProcessW 的 Unicode environment block 必须以额外 NUL 结束。
        block.push(0);
        Ok(Self {
            block,
            variable_count: entries.len(),
            path_entry_count: path_projection.len(),
        })
    }

    /// 返回只在 `CreateProcessW` 调用期间借用的完整 block 指针。
    fn block_ptr(&self) -> *const c_void {
        self.block.as_ptr().cast()
    }
}

impl fmt::Debug for ProviderChildEnvironment {
    /// 诊断只提供计数，不暴露 PATH、完整环境或 credential。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderChildEnvironment")
            .field("variable_count", &self.variable_count)
            .field("path_entry_count", &self.path_entry_count)
            .field("contents", &"<redacted>")
            .finish()
    }
}

/// 已绑定 CB6-001 resolved LaunchSpec、Workspace 投影与 Provider environment 的请求。
pub(crate) struct LaunchRequest {
    executable: PathBuf,
    args: Vec<OsString>,
    current_dir: ExternalProcessPath,
    environment: ProviderChildEnvironment,
    runtime_instance_id: String,
}

impl LaunchRequest {
    /// 从 frozen CB6-001 结果构造请求；不启动 ACP 或创建任何进程。
    pub(crate) fn from_resolved(
        resolved: &ResolvedLaunchSpec,
        frozen_canonical_root: &Path,
        unc_policy: UncCurrentDirectoryPolicy,
        runtime_instance_id: String,
    ) -> Result<Self, LaunchError> {
        validate_resolved_launch_spec(resolved)?;
        let current_dir = ExternalProcessPath::verify(frozen_canonical_root, unc_policy)?;
        let environment = ProviderChildEnvironment::from_host(&resolved.path_projection)?;
        let mut args = resolved.args.clone();
        if is_node_executable(&resolved.executable) {
            // ResolvedLaunchSpec 保留 canonical identity；只把 Node 主脚本 argv 投影给外部进程。
            args[0] = project_external_script_path(Path::new(&args[0]))?.into_os_string();
        }
        let request = Self {
            executable: resolved.executable.clone(),
            args,
            current_dir,
            environment,
            runtime_instance_id,
        };
        validate_request(&request)?;
        Ok(request)
    }

    /// 返回同一个已验证路径对象，供后续 session/new 接线复用。
    pub(crate) fn projected_cwd(&self) -> &ExternalProcessPath {
        &self.current_dir
    }

    /// 持久化 Runtime 与真实 Job 名必须引用同一个 R1。
    pub(crate) fn runtime_instance_id(&self) -> &str {
        &self.runtime_instance_id
    }
}

impl fmt::Debug for LaunchRequest {
    /// 不在诊断中展开 argv 或 environment，避免泄露 token/credential/PATH。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LaunchRequest")
            .field("executable", &self.executable)
            .field("arg_count", &self.args.len())
            .field("current_dir", &self.current_dir)
            .field("environment", &self.environment)
            .field("runtime_instance_id", &self.runtime_instance_id)
            .finish()
    }
}

/// CreateProcessW 成功后的唯一 owner；Job close 是内核策略，不是终止 evidence。
#[derive(Debug)]
pub(crate) struct CreatedChild {
    pub(crate) stdin: File,
    pub(crate) stdout: File,
    pub(crate) stderr: File,
    pub(crate) pid: u32,
    pub(super) process: OwnedHandle,
    pub(super) job: OwnedHandle,
}

/// 成功创建并读取了稳定 process creation token 的结果。
#[derive(Debug)]
pub(crate) struct LaunchedChild {
    pub(crate) child: CreatedChild,
    pub(crate) creation_filetime: u64,
}

impl LaunchedChild {
    /// 将原始 FILETIME 投影为固定 16 位十六进制 token。
    pub(crate) fn process_start_token(&self) -> String {
        format!("{:016x}", self.creation_filetime)
    }
}

/// launcher 的稳定、脱敏错误；`created` 表示 CreateProcessW 已成功。
#[derive(Debug)]
pub(crate) struct LaunchError {
    pub(crate) code: &'static str,
    pub(crate) win32_error: u32,
    /// caller 必须把该 owner 交给未来 Runtime 的终止/证据边界，不能只杀主 PID。
    pub(crate) created: Option<Box<CreatedChild>>,
}

impl fmt::Display for LaunchError {
    /// 只输出稳定码和 Win32 数字，不包含环境、PATH、argv 或 credential。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} (Win32 {})", self.code, self.win32_error)
    }
}

impl std::error::Error for LaunchError {}

/// 构造尚未创建进程的稳定错误。
fn failure(code: &'static str, win32_error: u32) -> LaunchError {
    LaunchError {
        code,
        win32_error,
        created: None,
    }
}

/// 将 frozen canonical root 投影为普通 Win32 local/UNC 路径。
fn project_external_path(
    canonical_root: &Path,
    unc_policy: UncCurrentDirectoryPolicy,
) -> Result<PathBuf, LaunchError> {
    let wide: Vec<_> = canonical_root.as_os_str().encode_wide().collect();
    let verbatim_unc = ascii_prefix_ignore_case(&wide, &wide_ascii(r"\\?\UNC\"));
    let verbatim = ascii_prefix_ignore_case(&wide, &wide_ascii(r"\\?\"));
    let mut projected = if verbatim_unc {
        let mut ordinary = vec![u16::from(b'\\'), u16::from(b'\\')];
        ordinary.extend_from_slice(&wide[8..]);
        PathBuf::from(OsString::from_wide(&ordinary))
    } else if verbatim {
        let remainder = &wide[4..];
        if remainder.len() < 3
            || !(u16::from(b'A')..=u16::from(b'Z')).contains(&remainder[0])
                && !(u16::from(b'a')..=u16::from(b'z')).contains(&remainder[0])
            || remainder[1] != u16::from(b':')
            || !matches!(remainder[2], value if value == u16::from(b'\\') || value == u16::from(b'/'))
        {
            return Err(failure(INVALID_WORKSPACE_PATH, ERROR_BAD_PATHNAME));
        }
        PathBuf::from(OsString::from_wide(remainder))
    } else {
        canonical_root.to_owned()
    };

    let projected_wide: Vec<_> = projected.as_os_str().encode_wide().collect();
    let is_unc = projected_wide.starts_with(&[u16::from(b'\\'), u16::from(b'\\')]);
    if is_unc && unc_policy == UncCurrentDirectoryPolicy::Unsupported {
        return Err(failure(UNSUPPORTED_UNC_CWD, ERROR_BAD_NETPATH));
    }
    if !projected.is_absolute() {
        return Err(failure(INVALID_WORKSPACE_PATH, ERROR_BAD_PATHNAME));
    }
    // 只规范分隔符表示，不做盘符映射，也不重新定义 canonical authority。
    if projected.as_os_str().is_empty() {
        projected = canonical_root.to_owned();
    }
    Ok(projected)
}

/// 将 canonical Node 主脚本投影为 Node 可消费的普通本地盘符路径。
fn project_external_script_path(canonical_script: &Path) -> Result<PathBuf, LaunchError> {
    let wide: Vec<_> = canonical_script.as_os_str().encode_wide().collect();
    let verbatim_unc = ascii_prefix_ignore_case(&wide, &wide_ascii(r"\\?\UNC\"));
    let verbatim = ascii_prefix_ignore_case(&wide, &wide_ascii(r"\\?\"));
    let ordinary_unc = wide.starts_with(&[u16::from(b'\\'), u16::from(b'\\')]);
    if verbatim_unc || (!verbatim && ordinary_unc) {
        return Err(failure(INVALID_SCRIPT_PATH, ERROR_BAD_NETPATH));
    }

    let projected = if verbatim {
        let remainder = &wide[4..];
        if !is_local_drive_absolute(remainder) {
            return Err(failure(INVALID_SCRIPT_PATH, ERROR_BAD_PATHNAME));
        }
        PathBuf::from(OsString::from_wide(remainder))
    } else {
        if !is_local_drive_absolute(&wide) {
            return Err(failure(INVALID_SCRIPT_PATH, ERROR_BAD_PATHNAME));
        }
        canonical_script.to_owned()
    };

    verify_projected_file_identity(canonical_script, &projected)?;
    Ok(projected)
}

/// 验证 UTF-16 路径是带根目录的本地盘符绝对路径。
fn is_local_drive_absolute(wide: &[u16]) -> bool {
    wide.len() >= 3
        && ((u16::from(b'A')..=u16::from(b'Z')).contains(&wide[0])
            || (u16::from(b'a')..=u16::from(b'z')).contains(&wide[0]))
        && wide[1] == u16::from(b':')
        && matches!(wide[2], value if value == u16::from(b'\\') || value == u16::from(b'/'))
}

/// 重新 canonicalize 投影前后的 regular file，并确认二者仍是同一 identity。
fn verify_projected_file_identity(frozen_file: &Path, projected: &Path) -> Result<(), LaunchError> {
    let frozen_canonical = canonicalize_regular_file(frozen_file)?;
    let projected_canonical = canonicalize_regular_file(projected)?;
    if !same_workspace_root_identity(&frozen_canonical, &projected_canonical) {
        return Err(failure(INVALID_SCRIPT_PATH, ERROR_BAD_PATHNAME));
    }
    Ok(())
}

/// canonicalize 单个 regular file；缺失、目录或不可验证路径一律 fail-closed。
fn canonicalize_regular_file(path: &Path) -> Result<PathBuf, LaunchError> {
    let metadata =
        fs::metadata(path).map_err(|_| failure(INVALID_SCRIPT_PATH, ERROR_BAD_PATHNAME))?;
    if !metadata.is_file() {
        return Err(failure(INVALID_SCRIPT_PATH, ERROR_BAD_PATHNAME));
    }
    fs::canonicalize(path).map_err(|_| failure(INVALID_SCRIPT_PATH, ERROR_BAD_PATHNAME))
}

/// 验证 resolver 产出的 executable 是可直接 CreateProcessW 的绝对边界。
fn validate_resolved_launch_spec(resolved: &ResolvedLaunchSpec) -> Result<(), LaunchError> {
    validate_direct_executable(&resolved.executable)?;
    let acp = OsStr::new("--acp");
    if is_node_executable(&resolved.executable) {
        let [script, flag] = resolved.args.as_slice() else {
            return Err(failure(INVALID_INPUT, ERROR_INVALID_PARAMETER));
        };
        let script = Path::new(script);
        let wrapper_extension = script.extension().is_some_and(|extension| {
            ["cmd", "bat", "ps1"]
                .iter()
                .any(|candidate| os_eq_ignore_case(extension, OsStr::new(candidate)))
        });
        if !script.is_absolute() || wrapper_extension || !os_eq_ignore_case(flag, acp) {
            return Err(failure(INVALID_INPUT, ERROR_INVALID_PARAMETER));
        }
    } else if resolved.args.len() != 1 || !os_eq_ignore_case(&resolved.args[0], acp) {
        // direct CodeBuddy executable 只接受 frozen ACP argv，不接收 shell/wrapper 参数。
        return Err(failure(INVALID_INPUT, ERROR_INVALID_PARAMETER));
    }
    Ok(())
}

/// 判断 frozen executable 是否为 npm wrapper 解析得到的 Node host。
fn is_node_executable(executable: &Path) -> bool {
    executable
        .file_name()
        .is_some_and(|name| os_eq_ignore_case(name, OsStr::new("node.exe")))
}

/// 验证 executable 是绝对 `.exe`/`.com`，并独立拒绝所有 wrapper/shim。
fn validate_direct_executable(executable: &Path) -> Result<(), LaunchError> {
    if !executable.is_absolute() {
        return Err(failure(INVALID_INPUT, ERROR_INVALID_PARAMETER));
    }
    let Some(extension) = executable.extension() else {
        return Err(failure(INVALID_INPUT, ERROR_INVALID_PARAMETER));
    };
    if !os_eq_ignore_case(extension, OsStr::new("exe"))
        && !os_eq_ignore_case(extension, OsStr::new("com"))
    {
        return Err(failure(INVALID_INPUT, ERROR_INVALID_PARAMETER));
    }
    Ok(())
}

/// 验证仅由内部构造器建立的 request 不变量。
fn validate_request(request: &LaunchRequest) -> Result<(), LaunchError> {
    validate_direct_executable(&request.executable)?;
    if !request.current_dir.as_path().is_absolute()
        || request.runtime_instance_id.is_empty()
        || request.runtime_instance_id.contains(['\\', '/', '\0'])
    {
        return Err(failure(INVALID_INPUT, ERROR_INVALID_PARAMETER));
    }
    Ok(())
}

/// 验证单个环境变量可安全编码进 Windows block。
fn validate_environment_entry(name: &OsStr, value: &OsStr) -> Result<(), LaunchError> {
    let name: Vec<_> = name.encode_wide().collect();
    let value: Vec<_> = value.encode_wide().collect();
    // Windows 会在 Host block 中保留 `=C:` 这类盘符 current-directory 项；
    // 它们只有首字符可为 `=`，普通变量名和其余位置仍禁止分隔符。
    let invalid_separator = if name.first() == Some(&u16::from(b'=')) {
        name[1..].contains(&u16::from(b'='))
    } else {
        name.contains(&u16::from(b'='))
    };
    if name.is_empty()
        || name.contains(&0)
        || invalid_separator
        || value.contains(&0)
        || name.len() > i32::MAX as usize
    {
        return Err(failure(INVALID_INPUT, ERROR_INVALID_PARAMETER));
    }
    Ok(())
}

/// 使用 Windows ordinal ignore-case 语义比较环境变量名。
fn compare_os_ignore_case(left: &OsStr, right: &OsStr) -> Ordering {
    let left_wide: Vec<_> = left.encode_wide().collect();
    let right_wide: Vec<_> = right.encode_wide().collect();
    let result = unsafe {
        CompareStringOrdinal(
            left_wide.as_ptr(),
            left_wide.len() as i32,
            right_wide.as_ptr(),
            right_wide.len() as i32,
            1,
        )
    };
    match result {
        CSTR_LESS_THAN => Ordering::Less,
        CSTR_GREATER_THAN => Ordering::Greater,
        CSTR_EQUAL => left_wide.cmp(&right_wide),
        _ => left_wide.cmp(&right_wide),
    }
}

/// 判断两个 OS 字符串在 Windows ordinal ignore-case 下是否相等。
fn os_eq_ignore_case(left: &OsStr, right: &OsStr) -> bool {
    let left_wide: Vec<_> = left.encode_wide().collect();
    let right_wide: Vec<_> = right.encode_wide().collect();
    unsafe {
        CompareStringOrdinal(
            left_wide.as_ptr(),
            left_wide.len() as i32,
            right_wide.as_ptr(),
            right_wide.len() as i32,
            1,
        ) == CSTR_EQUAL
    }
}

/// 将固定 ASCII prefix 转为 UTF-16，避免通过有损 UTF-8 处理 Windows path。
fn wide_ascii(value: &str) -> Vec<u16> {
    value.bytes().map(u16::from).collect()
}

/// 以 ASCII 大小写不敏感方式匹配 Windows namespace prefix。
fn ascii_prefix_ignore_case(value: &[u16], prefix: &[u16]) -> bool {
    value.len() >= prefix.len()
        && value.iter().zip(prefix).all(|(left, right)| {
            let left = if (u16::from(b'a')..=u16::from(b'z')).contains(left) {
                left - u16::from(b'a') + u16::from(b'A')
            } else {
                *left
            };
            let right = if (u16::from(b'a')..=u16::from(b'z')).contains(right) {
                right - u16::from(b'a') + u16::from(b'A')
            } else {
                *right
            };
            left == right
        })
}

/// 编码 NUL-terminated UTF-16 字符串。
fn wide(value: &OsStr) -> Result<Vec<u16>, LaunchError> {
    let mut result: Vec<_> = value.encode_wide().collect();
    if result.contains(&0) {
        return Err(failure(INVALID_INPUT, ERROR_INVALID_PARAMETER));
    }
    result.push(0);
    Ok(result)
}

/// 按 Microsoft CRT 规则编码 argv，避免 shell string 与 UTF-8 有损转换。
fn command_line(executable: &OsStr, args: &[OsString]) -> Result<Vec<u16>, LaunchError> {
    let mut result = Vec::new();
    for (index, argument) in std::iter::once(executable)
        .chain(args.iter().map(OsString::as_os_str))
        .enumerate()
    {
        if index != 0 {
            result.push(u16::from(b' '));
        }
        result.push(u16::from(b'"'));
        let mut slashes = 0;
        for unit in argument.encode_wide() {
            if unit == 0 {
                return Err(failure(INVALID_INPUT, ERROR_INVALID_PARAMETER));
            }
            if unit == u16::from(b'\\') {
                slashes += 1;
                continue;
            }
            result.extend(std::iter::repeat_n(
                u16::from(b'\\'),
                if unit == u16::from(b'"') {
                    slashes * 2 + 1
                } else {
                    slashes
                },
            ));
            slashes = 0;
            result.push(unit);
        }
        result.extend(std::iter::repeat_n(u16::from(b'\\'), slashes * 2));
        result.push(u16::from(b'"'));
    }
    result.push(0);
    if result.len() > 32767 {
        return Err(failure(INVALID_INPUT, ERROR_INVALID_PARAMETER));
    }
    Ok(result)
}

/// 持有初始化后的双 attribute list storage。
struct Attributes {
    storage: Vec<usize>,
}

impl Attributes {
    /// 为 JOB_LIST 与 HANDLE_LIST 初始化 attribute list。
    fn new() -> Result<Self, LaunchError> {
        let mut bytes = 0;
        // SAFETY: 第一次调用只探测大小，第二次使用 pointer-aligned storage。
        unsafe {
            let result = InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes);
            let error = GetLastError();
            if result != 0 || error != ERROR_INSUFFICIENT_BUFFER || bytes == 0 {
                return Err(failure("CODEBUDDY_JOB_AT_CREATION_UNSUPPORTED", error));
            }
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            if InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 2, 0, &mut bytes) == 0
            {
                return Err(failure(
                    "CODEBUDDY_JOB_AT_CREATION_UNSUPPORTED",
                    GetLastError(),
                ));
            }
            Ok(Self { storage })
        }
    }

    /// 返回在 storage 生命周期内有效的 attribute list 指针。
    fn ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for Attributes {
    /// 释放已成功初始化的 attribute list 内部状态。
    fn drop(&mut self) {
        // SAFETY: 类型只在初始化成功后构造，storage 在调用期间仍然存活。
        unsafe { DeleteProcThreadAttributeList(self.ptr()) };
    }
}

/// 创建 pipe，并只让 Child 端具备 inheritable flag。
fn pipe(child_reads: bool) -> Result<(OwnedHandle, OwnedHandle), LaunchError> {
    let (mut read, mut write) = (null_mut(), null_mut());
    // SAFETY: 输出指针有效；成功句柄立即且只被 OwnedHandle 接管一次。
    unsafe {
        if CreatePipe(&mut read, &mut write, null(), 0) == 0 {
            return Err(failure("CODEBUDDY_PIPE_CREATE_FAILED", GetLastError()));
        }
        let read = OwnedHandle::from_raw_handle(read);
        let write = OwnedHandle::from_raw_handle(write);
        let (child, parent) = if child_reads {
            (read, write)
        } else {
            (write, read)
        };
        if SetHandleInformation(
            child.as_raw_handle(),
            HANDLE_FLAG_INHERIT,
            HANDLE_FLAG_INHERIT,
        ) == 0
            || SetHandleInformation(parent.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) == 0
        {
            return Err(failure("CODEBUDDY_HANDLE_POLICY_FAILED", GetLastError()));
        }
        Ok((child, parent))
    }
}

/// 查询并验证 Job 的 KILL_ON_CLOSE 与 no-breakaway policy。
fn validate_job_policy(job: HANDLE) -> Result<(), LaunchError> {
    let mut handle_flags = 0;
    // SAFETY: `job` 在调用期间由 OwnedHandle 持有，flags 输出指针有效。
    if unsafe { GetHandleInformation(job, &mut handle_flags) } == 0 {
        return Err(failure("CODEBUDDY_JOB_POLICY_FAILED", unsafe {
            GetLastError()
        }));
    }
    if handle_flags & HANDLE_FLAG_INHERIT != 0 {
        return Err(failure(
            "CODEBUDDY_JOB_POLICY_INVALID",
            ERROR_INVALID_PARAMETER,
        ));
    }
    let mut policy: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    // SAFETY: buffer 类型、长度与查询 class 一致，Job handle 由 caller 持有。
    if unsafe {
        QueryInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&mut policy as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            null_mut(),
        )
    } == 0
    {
        return Err(failure("CODEBUDDY_JOB_POLICY_FAILED", unsafe {
            GetLastError()
        }));
    }
    let flags = policy.BasicLimitInformation.LimitFlags;
    let forbidden = JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
    if flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE == 0 || flags & forbidden != 0 {
        return Err(failure(
            "CODEBUDDY_JOB_POLICY_INVALID",
            ERROR_INVALID_PARAMETER,
        ));
    }
    Ok(())
}

/// 从第一个可运行时刻把直接 executable 放入 Job；caller 应在 blocking worker 调用。
pub(crate) fn launch(request: &LaunchRequest) -> Result<LaunchedChild, LaunchError> {
    launch_inner(
        request,
        #[cfg(test)]
        None,
    )
}

/// 执行 launcher；测试可以注入精确 acquisition checkpoint。
fn launch_inner(
    request: &LaunchRequest,
    #[cfg(test)] fault: Option<Checkpoint>,
) -> Result<LaunchedChild, LaunchError> {
    validate_request(request)?;
    let executable = wide(request.executable.as_os_str())?;
    let directory = wide(request.current_dir.as_path().as_os_str())?;
    let mut command = command_line(request.executable.as_os_str(), &request.args)?;
    let job_name = wide(OsStr::new(&format!(
        "Local\\SerenaDesktop.CodeBuddy.{}",
        request.runtime_instance_id
    )))?;

    // SAFETY: 所有输入 buffer 在调用期间存活；每个成功句柄立即被唯一 owner 接管。
    unsafe {
        let raw_job = CreateJobObjectW(null(), job_name.as_ptr());
        let create_job_error = GetLastError();
        if raw_job.is_null() {
            return Err(failure("CODEBUDDY_JOB_CREATE_FAILED", create_job_error));
        }
        let job = OwnedHandle::from_raw_handle(raw_job);
        if create_job_error == ERROR_ALREADY_EXISTS {
            return Err(failure("CODEBUDDY_JOB_NAME_COLLISION", create_job_error));
        }
        #[cfg(test)]
        fail_at(fault, Checkpoint::JobCreated)?;

        if SetHandleInformation(job.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(failure("CODEBUDDY_HANDLE_POLICY_FAILED", GetLastError()));
        }
        let mut policy: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        policy.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&policy as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            return Err(failure("CODEBUDDY_JOB_POLICY_FAILED", GetLastError()));
        }
        validate_job_policy(job.as_raw_handle())?;
        #[cfg(test)]
        fail_at(fault, Checkpoint::JobConfigured)?;

        let (stdin_read, stdin_write) = pipe(true)?;
        #[cfg(test)]
        fail_at(fault, Checkpoint::StdinCreated)?;
        let (stdout_write, stdout_read) = pipe(false)?;
        #[cfg(test)]
        fail_at(fault, Checkpoint::StdoutCreated)?;
        let (stderr_write, stderr_read) = pipe(false)?;
        #[cfg(test)]
        fail_at(fault, Checkpoint::StderrCreated)?;

        // attribute values 与 list 一起保持到 CreateProcessW 返回。
        let jobs = [job.as_raw_handle()];
        let handles = [
            stdin_read.as_raw_handle(),
            stdout_write.as_raw_handle(),
            stderr_write.as_raw_handle(),
        ];
        let mut attributes = Attributes::new()?;
        #[cfg(test)]
        fail_at(fault, Checkpoint::AttributesInitialized)?;
        if UpdateProcThreadAttribute(
            attributes.ptr(),
            0,
            PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
            jobs.as_ptr().cast(),
            size_of_val(&jobs),
            null_mut(),
            null(),
        ) == 0
        {
            return Err(failure(
                "CODEBUDDY_JOB_AT_CREATION_UNSUPPORTED",
                GetLastError(),
            ));
        }
        #[cfg(test)]
        fail_at(fault, Checkpoint::JobAttributeSet)?;
        if UpdateProcThreadAttribute(
            attributes.ptr(),
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_ptr().cast(),
            size_of_val(&handles),
            null_mut(),
            null(),
        ) == 0
        {
            return Err(failure("CODEBUDDY_HANDLE_POLICY_FAILED", GetLastError()));
        }
        #[cfg(test)]
        fail_at(fault, Checkpoint::HandleAttributeSet)?;

        let mut startup: STARTUPINFOEXW = zeroed();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        startup.lpAttributeList = attributes.ptr();
        let mut info: PROCESS_INFORMATION = zeroed();
        if CreateProcessW(
            executable.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            request.environment.block_ptr(),
            directory.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        ) == 0
        {
            return Err(failure("CODEBUDDY_PROCESS_CREATE_FAILED", GetLastError()));
        }

        // CreateProcessW 成功后的第一步先接管两个 kernel handle，再执行任何可失败验证。
        let process = OwnedHandle::from_raw_handle(info.hProcess);
        let thread = OwnedHandle::from_raw_handle(info.hThread);
        drop((stdin_read, stdout_write, stderr_write, thread));
        let child = CreatedChild {
            stdin: File::from(stdin_write),
            stdout: File::from(stdout_read),
            stderr: File::from(stderr_read),
            pid: info.dwProcessId,
            process,
            job,
        };

        #[cfg(test)]
        if fault == Some(Checkpoint::ProcessCreated) {
            return Err(LaunchError {
                code: "CODEBUDDY_PROCESS_IDENTITY_FAILED",
                win32_error: ERROR_GEN_FAILURE,
                created: Some(Box::new(child)),
            });
        }
        let mut created: FILETIME = zeroed();
        let mut exited: FILETIME = zeroed();
        let mut kernel: FILETIME = zeroed();
        let mut user: FILETIME = zeroed();
        if GetProcessTimes(
            child.process.as_raw_handle(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        ) == 0
        {
            return Err(LaunchError {
                code: "CODEBUDDY_PROCESS_IDENTITY_FAILED",
                win32_error: GetLastError(),
                created: Some(Box::new(child)),
            });
        }
        Ok(LaunchedChild {
            child,
            creation_filetime: ((created.dwHighDateTime as u64) << 32)
                | created.dwLowDateTime as u64,
        })
    }
}

/// 测试专用 acquisition checkpoint。
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Checkpoint {
    JobCreated,
    JobConfigured,
    StdinCreated,
    StdoutCreated,
    StderrCreated,
    AttributesInitialized,
    JobAttributeSet,
    HandleAttributeSet,
    ProcessCreated,
}

/// 在指定 checkpoint 注入无副作用失败。
#[cfg(test)]
fn fail_at(fault: Option<Checkpoint>, checkpoint: Checkpoint) -> Result<(), LaunchError> {
    if fault == Some(checkpoint) {
        Err(failure("TEST_INJECTED_API_FAILURE", ERROR_GEN_FAILURE))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
