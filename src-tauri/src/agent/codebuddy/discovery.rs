//! CodeBuddy Code ACP CLI 的无进程 Windows discovery。

use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    fs::{self, File},
    path::{Path, PathBuf},
};

const CODEBUDDY_NOT_FOUND: &str = "CODEBUDDY_BINARY_NOT_FOUND";
const PACKAGE_SCRIPT: &str = "node_modules/@tencent-ai/codebuddy-code/bin/codebuddy";
const MAX_WRAPPER_BYTES: u64 = 64 * 1024;
const MAX_PACKAGE_METADATA_BYTES: u64 = 64 * 1024;

/// PATH entry 的安全来源标识；不会携带原始 PATH 内容。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiscoverySource {
    Explicit,
    Process,
    RegistryCurrentUser,
    RegistryLocalMachine,
    SafeCommon,
}

/// best-effort 版本元数据状态；异常不能改变 admission 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetadataStatus {
    Missing,
    Parsed,
    Malformed,
}

/// 仅用于诊断和 Catalog 展示的版本元数据，不是 compatibility authority。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VersionMetadata {
    pub(crate) product_version: Option<String>,
    pub(crate) base_version: Option<String>,
    pub(crate) package_version: Option<String>,
    pub(crate) status: MetadataStatus,
}

impl VersionMetadata {
    /// 构造没有可用元数据的安全默认值。
    fn missing() -> Self {
        Self {
            product_version: None,
            base_version: None,
            package_version: None,
            status: MetadataStatus::Missing,
        }
    }

    /// 构造不可解析元数据；不保留原始内容或错误文本。
    fn malformed() -> Self {
        Self {
            product_version: None,
            base_version: None,
            package_version: None,
            status: MetadataStatus::Malformed,
        }
    }
}

/// 后续 Job-at-creation launcher 消费的本机解析结果，不等同于 Catalog 默认描述。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ResolvedLaunchSpec {
    pub(crate) executable: PathBuf,
    pub(crate) args: Vec<OsString>,
    pub(crate) path_projection: Vec<PathBuf>,
}

/// 可公开到安全诊断的最小 provenance；不包含完整 PATH、env 或 wrapper 正文。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiscoveryProvenance {
    pub(crate) source: DiscoverySource,
    pub(crate) resolved_executable: PathBuf,
    pub(crate) wrapper_resolved: bool,
    pub(crate) metadata_status: MetadataStatus,
}

/// 一次成功 discovery 的完整内部结果。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct DiscoveryResult {
    pub(crate) launch_spec: ResolvedLaunchSpec,
    pub(crate) provenance: DiscoveryProvenance,
    pub(crate) metadata: VersionMetadata,
}

impl DiscoveryResult {
    /// 构造不接触文件系统的测试结果，用于验证 Registry refresh 的控制面语义。
    #[cfg(test)]
    pub(crate) fn direct_for_test(executable: impl Into<PathBuf>) -> Self {
        let executable = executable.into();
        Self {
            launch_spec: ResolvedLaunchSpec {
                executable: executable.clone(),
                args: vec!["--acp".into()],
                path_projection: Vec::new(),
            },
            provenance: DiscoveryProvenance {
                source: DiscoverySource::Explicit,
                resolved_executable: executable,
                wrapper_resolved: false,
                metadata_status: MetadataStatus::Missing,
            },
            metadata: VersionMetadata::missing(),
        }
    }
}

/// discovery 失败只保留稳定码和 IDE hint，不泄露被扫描的 PATH。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiscoveryError {
    pub(crate) diagnostic_code: &'static str,
    pub(crate) buddycn_detected: bool,
}

impl DiscoveryError {
    /// 构造稳定的缺失诊断；`buddycn` 仅作为提示，不能升级为 executable。
    pub(crate) fn not_found(buddycn_detected: bool) -> Self {
        Self {
            diagnostic_code: CODEBUDDY_NOT_FOUND,
            buddycn_detected,
        }
    }
}

/// resolver 的 crate-private 输入边界；当前生产配置不提供 explicit PATH。
#[derive(Clone, Default)]
pub(crate) struct DiscoveryInput {
    pub(crate) explicit_executable: Option<PathBuf>,
    pub(crate) explicit_path: Option<OsString>,
    pub(crate) process_path: Option<OsString>,
    pub(crate) registry_current_user_path: Option<OsString>,
    pub(crate) registry_local_machine_path: Option<OsString>,
    pub(crate) environment: Vec<(OsString, OsString)>,
    pub(crate) safe_common_dirs: Vec<PathBuf>,
}

impl DiscoveryInput {
    /// 每次调用重新读取 Desktop 当前环境和 Registry，避免缓存陈旧 PATH。
    pub(crate) fn system() -> Self {
        let environment: Vec<_> = std::env::vars_os().collect();
        let (registry_current_user_path, registry_local_machine_path) = registry_paths();
        let safe_common_dirs = safe_common_dirs(&environment);
        Self {
            explicit_executable: None,
            explicit_path: None,
            process_path: std::env::var_os("PATH"),
            registry_current_user_path,
            registry_local_machine_path,
            environment,
            safe_common_dirs,
        }
    }
}

/// 一个已展开且带来源标记的内部 PATH entry。
#[derive(Clone)]
struct PathEntry {
    path: PathBuf,
    source: DiscoverySource,
}

/// 按冻结优先级解析 CodeBuddy Code CLI，整个过程只进行环境、Registry 和文件读取。
pub(crate) fn discover(input: DiscoveryInput) -> Result<DiscoveryResult, DiscoveryError> {
    let entries = project_paths(&input);
    let projection: Vec<_> = entries.iter().map(|entry| entry.path.clone()).collect();
    if let Some(candidate) = input.explicit_executable.as_deref()
        && let Some(resolved) = resolve_candidate(candidate, &entries)
    {
        return Ok(resolved_result(
            DiscoverySource::Explicit,
            projection,
            resolved,
        ));
    }
    for entry in &entries {
        for extension in ["exe", "com", "cmd", "bat"] {
            let candidate = entry.path.join(format!("codebuddy.{extension}"));
            if let Some(resolved) = resolve_candidate(&candidate, &entries) {
                return Ok(resolved_result(entry.source, projection, resolved));
            }
        }
    }
    Err(DiscoveryError::not_found(buddycn_exists(&entries)))
}

/// 将内部 candidate 解析结果投影为与 default descriptor 分离的 LaunchSpec。
fn resolved_result(
    source: DiscoverySource,
    path_projection: Vec<PathBuf>,
    resolved: (PathBuf, Vec<OsString>, bool, VersionMetadata),
) -> DiscoveryResult {
    let (executable, args, wrapper_resolved, metadata) = resolved;
    DiscoveryResult {
        launch_spec: ResolvedLaunchSpec {
            executable: executable.clone(),
            args,
            path_projection,
        },
        provenance: DiscoveryProvenance {
            source,
            resolved_executable: executable,
            wrapper_resolved,
            metadata_status: metadata.status,
        },
        metadata,
    }
}

/// 将来源 PATH 合成为稳定 projection，并按 Windows 大小写规则保留首次出现项。
fn project_paths(input: &DiscoveryInput) -> Vec<PathEntry> {
    let environment = environment_map(&input.environment);
    let mut entries = Vec::new();
    append_path_value(
        &mut entries,
        input.explicit_path.as_deref(),
        DiscoverySource::Explicit,
        &environment,
    );
    append_path_value(
        &mut entries,
        input.process_path.as_deref(),
        DiscoverySource::Process,
        &environment,
    );
    append_path_value(
        &mut entries,
        input.registry_current_user_path.as_deref(),
        DiscoverySource::RegistryCurrentUser,
        &environment,
    );
    append_path_value(
        &mut entries,
        input.registry_local_machine_path.as_deref(),
        DiscoverySource::RegistryLocalMachine,
        &environment,
    );
    for path in &input.safe_common_dirs {
        entries.push(PathEntry {
            path: path.clone(),
            source: DiscoverySource::SafeCommon,
        });
    }

    let mut seen = Vec::<PathBuf>::new();
    entries.retain(|entry| {
        if seen
            .iter()
            .any(|existing| same_windows_path(existing, &entry.path))
        {
            false
        } else {
            seen.push(entry.path.clone());
            true
        }
    });
    entries
}

/// 构建大小写不敏感的环境 lookup，不将该 map 写入结果或诊断。
fn environment_map(values: &[(OsString, OsString)]) -> HashMap<String, OsString> {
    values
        .iter()
        .map(|(key, value)| (key.to_string_lossy().to_ascii_lowercase(), value.clone()))
        .collect()
}

/// 展开一个 PATH 来源并追加非空目录；无法展开的变量保持原文本并自然 miss。
fn append_path_value(
    entries: &mut Vec<PathEntry>,
    value: Option<&OsStr>,
    source: DiscoverySource,
    environment: &HashMap<String, OsString>,
) {
    let Some(value) = value else { return };
    let expanded = expand_percent_variables(value, environment);
    for path in std::env::split_paths(&expanded) {
        let text = path.to_string_lossy();
        let trimmed = text.trim().trim_matches('"');
        if !trimmed.is_empty() {
            entries.push(PathEntry {
                path: PathBuf::from(trimmed),
                source,
            });
        }
    }
}

/// 以 Windows `%NAME%` 语义展开已知变量；匹配不区分大小写。
fn expand_percent_variables(value: &OsStr, environment: &HashMap<String, OsString>) -> OsString {
    let text = value.to_string_lossy();
    if !text.contains('%') {
        return value.to_os_string();
    }
    let mut output = String::with_capacity(text.len());
    let mut remainder = text.as_ref();
    while let Some(start) = remainder.find('%') {
        output.push_str(&remainder[..start]);
        let after_start = &remainder[start + 1..];
        let Some(end) = after_start.find('%') else {
            output.push_str(&remainder[start..]);
            remainder = "";
            break;
        };
        let name = &after_start[..end];
        if let Some(replacement) = environment.get(&name.to_ascii_lowercase()) {
            output.push_str(&replacement.to_string_lossy());
        } else {
            output.push('%');
            output.push_str(name);
            output.push('%');
        }
        remainder = &after_start[end + 1..];
    }
    output.push_str(remainder);
    output.into()
}

/// 使用 Windows UTF-16 ordinal ignore-case 逐组件比较，避免有损字符串和 drive-root 混淆。
#[cfg(windows)]
fn same_windows_path(left: &Path, right: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

    let mut left = left.components();
    let mut right = right.components();
    loop {
        match (left.next(), right.next()) {
            (None, None) => return true,
            (Some(left), Some(right)) => {
                let left = left.as_os_str().encode_wide().collect::<Vec<_>>();
                let right = right.as_os_str().encode_wide().collect::<Vec<_>>();
                let (Ok(left_len), Ok(right_len)) =
                    (i32::try_from(left.len()), i32::try_from(right.len()))
                else {
                    return false;
                };
                // 显式长度允许 UTF-16 buffer 不以 NUL 结尾。
                let equal = unsafe {
                    CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1)
                        == CSTR_EQUAL
                };
                if !equal {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// 非 Windows 构建只保留可测试的 fallback；生产 resolver 的语义由 Windows 实现定义。
#[cfg(not(windows))]
fn same_windows_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy().replace('/', "\\").to_lowercase()
        == right.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// 解析 direct executable 或受支持的 npm wrapper；无法安全解析时继续搜索。
fn resolve_candidate(
    candidate: &Path,
    entries: &[PathEntry],
) -> Option<(PathBuf, Vec<OsString>, bool, VersionMetadata)> {
    if !readable_file(candidate) {
        return None;
    }
    let extension = candidate
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase();
    match extension.as_str() {
        "exe" | "com" => {
            let executable = fs::canonicalize(candidate).ok()?;
            Some((
                executable,
                vec!["--acp".into()],
                false,
                VersionMetadata::missing(),
            ))
        }
        "cmd" | "bat" => resolve_npm_wrapper(candidate, entries),
        _ => None,
    }
}

/// 将 npm wrapper 解析为真实 node.exe、CLI script 和 `--acp` argv。
fn resolve_npm_wrapper(
    wrapper: &Path,
    entries: &[PathEntry],
) -> Option<(PathBuf, Vec<OsString>, bool, VersionMetadata)> {
    let metadata = fs::metadata(wrapper).ok()?;
    if metadata.len() > MAX_WRAPPER_BYTES {
        return None;
    }
    let body = fs::read_to_string(wrapper).ok()?;
    let normalized = body.replace('\\', "/").to_ascii_lowercase();
    if !normalized.contains(PACKAGE_SCRIPT) {
        return None;
    }
    let root = wrapper.parent()?;
    let script = root.join(PACKAGE_SCRIPT);
    if !readable_file(&script) {
        return None;
    }
    let node = std::iter::once(root.to_path_buf())
        .chain(entries.iter().map(|entry| entry.path.clone()))
        .map(|directory| directory.join("node.exe"))
        .find(|candidate| readable_file(candidate))?;
    let executable = fs::canonicalize(node).ok()?;
    let script = fs::canonicalize(script).ok()?;
    let version = read_package_metadata(&script);
    Some((
        executable,
        vec![script.into_os_string(), "--acp".into()],
        true,
        version,
    ))
}

/// 只确认文件存在且可读，不执行或解释其内容。
fn readable_file(path: &Path) -> bool {
    path.is_file() && File::open(path).is_ok()
}

/// 读取 npm package 的有限元数据；异常只改变诊断状态。
fn read_package_metadata(script: &Path) -> VersionMetadata {
    let Some(package_root) = script.parent().and_then(Path::parent) else {
        return VersionMetadata::missing();
    };
    let package = package_root.join("package.json");
    let Ok(metadata) = fs::metadata(&package) else {
        return VersionMetadata::missing();
    };
    if metadata.len() > MAX_PACKAGE_METADATA_BYTES {
        return VersionMetadata::malformed();
    }
    let Ok(bytes) = fs::read(package) else {
        return VersionMetadata::malformed();
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return VersionMetadata::malformed();
    };
    // 字段保持来源语义：npm `version` 不能冒充 ProductVersion 或 baseVersion。
    let product = string_metadata(&value, "productVersion");
    let base = string_metadata(&value, "baseVersion");
    let package = string_metadata(&value, "version");
    if product.is_err() || base.is_err() || package.is_err() {
        return VersionMetadata::malformed();
    }
    let product_version = product.ok().flatten();
    let base_version = base.ok().flatten();
    let package_version = package.ok().flatten();
    let status = if product_version.is_some() || base_version.is_some() || package_version.is_some()
    {
        MetadataStatus::Parsed
    } else {
        MetadataStatus::Missing
    };
    VersionMetadata {
        product_version,
        base_version,
        package_version,
        status,
    }
}

/// 读取可选 string metadata；存在但不是非空 string 时标记异常。
fn string_metadata(value: &serde_json::Value, key: &str) -> Result<Option<String>, ()> {
    let Some(value) = value.get(key) else {
        return Ok(None);
    };
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| Some(value.to_owned()))
        .ok_or(())
}

/// 检测 IDE-only `buddycn`，仅返回 boolean hint，不解析或返回其路径。
fn buddycn_exists(entries: &[PathEntry]) -> bool {
    entries.iter().any(|entry| {
        ["exe", "com", "cmd", "bat"]
            .iter()
            .any(|extension| readable_file(&entry.path.join(format!("buddycn.{extension}"))))
    })
}

/// 从 Windows 用户/机器环境读取 PATH；读取失败按缺失处理。
#[cfg(windows)]
fn registry_paths() -> (Option<OsString>, Option<OsString>) {
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ},
    };
    let current_user = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ)
        .ok()
        .and_then(|key| key.get_value::<String, _>("Path").ok())
        .map(OsString::from);
    let local_machine = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
            KEY_READ,
        )
        .ok()
        .and_then(|key| key.get_value::<String, _>("Path").ok())
        .map(OsString::from);
    (current_user, local_machine)
}

/// 非 Windows 平台没有本卡定义的 Registry PATH 来源。
#[cfg(not(windows))]
fn registry_paths() -> (Option<OsString>, Option<OsString>) {
    (None, None)
}

/// 从标准环境变量导出有限 common dirs，不递归扫描或硬编码用户绝对路径。
fn safe_common_dirs(environment: &[(OsString, OsString)]) -> Vec<PathBuf> {
    let environment = environment_map(environment);
    let mut directories = Vec::new();
    if let Some(appdata) = environment.get("appdata") {
        directories.push(PathBuf::from(appdata).join("npm"));
    }
    if let Some(local) = environment.get("localappdata") {
        directories.push(PathBuf::from(local).join("Programs"));
    }
    for key in ["programfiles", "programfiles(x86)"] {
        if let Some(program_files) = environment.get(key) {
            directories.push(PathBuf::from(program_files).join("nodejs"));
        }
    }
    directories
}

#[cfg(test)]
mod tests;
