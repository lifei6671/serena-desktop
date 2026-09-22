use std::{
    collections::{BTreeMap, HashSet},
    fs,
    future::Future,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use super::app_server::managed::{ProbeFailure, ProbeRuntimeFailure};
use crate::agent::task_manager::ProbeContext;

const CPU_TYPE_X86_64: u32 = 0x0100_0007;
const CPU_TYPE_ARM64: u32 = 0x0100_000c;
const MACHO_64_MAGIC: u32 = 0xfeed_facf;
const FAT_MAGIC: u32 = 0xcafe_babe;
const FAT_MAGIC_64: u32 = 0xcafe_babf;
const MAX_MACHO_HEADER: usize = 64 * 1024;

/// 不依赖 shell 的 macOS Codex 候选枚举上下文。
#[derive(Clone, Debug)]
pub(crate) struct DiscoveryContext {
    pub(crate) path_entries: Vec<PathBuf>,
    pub(crate) home: Option<PathBuf>,
    pub(crate) fixed_prefixes: Vec<PathBuf>,
    pub(crate) chatgpt_candidate: PathBuf,
}

impl DiscoveryContext {
    /// 从 GUI 进程环境构造上下文；不会加载 shell dotfiles 或运行外部命令。
    fn current() -> Self {
        Self {
            path_entries: std::env::var_os("PATH")
                .map(|value| std::env::split_paths(&value).collect())
                .unwrap_or_default(),
            home: std::env::var_os("HOME").map(PathBuf::from),
            fixed_prefixes: vec![PathBuf::from("/opt/homebrew"), PathBuf::from("/usr/local")],
            chatgpt_candidate: PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex"),
        }
    }
}

/// Discovery 的稳定失败类别；消息只包含有界摘要，不回显完整环境。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiscoveryError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl DiscoveryError {
    /// 创建一个稳定类别的 discovery 失败。
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for DiscoveryError {
    /// 仅输出稳定错误码与已脱敏诊断，不展开候选路径或环境变量。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for DiscoveryError {}

/// 将候选失败压缩为不含路径和环境值的稳定、有界类别摘要。
fn failure_summary(failures: &BTreeMap<&'static str, usize>) -> String {
    failures
        .iter()
        .take(8)
        .map(|(code, count)| format!("{code}={count}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// 首版只允许原生 Apple Silicon macOS Host。
fn host_supported(os: &str, arch: &str) -> bool {
    os == "macos" && arch == "aarch64"
}

/// 按冻结来源顺序枚举所有可能路径，不把“存在”误当作“兼容”。
pub(crate) fn enumerate_candidates(context: &DiscoveryContext) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for entry in &context.path_entries {
        candidates.push(entry.join("codex"));
    }
    if let Some(home) = &context.home {
        candidates.push(home.join(".local/bin/codex"));
    }
    for prefix in &context.fixed_prefixes {
        candidates.push(prefix.join("bin/codex"));
    }

    let mut npm_prefixes = context.fixed_prefixes.clone();
    if let Some(home) = &context.home {
        npm_prefixes.push(home.join(".local"));
        npm_prefixes.push(home.join(".npm-global"));
    }
    for entry in &context.path_entries {
        if entry.file_name().is_some_and(|name| name == "bin")
            && let Some(prefix) = entry.parent()
        {
            npm_prefixes.push(prefix.to_owned());
        }
    }
    let mut seen_prefixes = HashSet::new();
    for prefix in npm_prefixes {
        if !seen_prefixes.insert(prefix.clone()) {
            continue;
        }
        let package = prefix.join("lib/node_modules/@openai/codex");
        candidates.push(
            package.join(
                "node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex",
            ),
        );
        candidates.push(package.join("vendor/aarch64-apple-darwin/bin/codex"));
    }
    candidates.push(context.chatgpt_candidate.clone());
    candidates
}

/// 读取有界 Mach-O header，确认至少包含一个 ARM64 slice。
fn macho_contains_arm64(path: &Path) -> Result<bool, DiscoveryError> {
    let mut file = fs::File::open(path)
        .map_err(|error| DiscoveryError::new("CODEX_EXECUTABLE_NOT_RUNNABLE", error.to_string()))?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_MACHO_HEADER as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            DiscoveryError::new("CODEX_EXECUTABLE_FORMAT_UNSUPPORTED", error.to_string())
        })?;
    if bytes.len() < 8 {
        return Err(DiscoveryError::new(
            "CODEX_EXECUTABLE_FORMAT_UNSUPPORTED",
            "Mach-O header is truncated",
        ));
    }

    let magic_le = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let magic_be = u32::from_be_bytes(bytes[0..4].try_into().unwrap());
    if magic_le == MACHO_64_MAGIC || magic_be == MACHO_64_MAGIC {
        let cpu = if magic_le == MACHO_64_MAGIC {
            u32::from_le_bytes(bytes[4..8].try_into().unwrap())
        } else {
            u32::from_be_bytes(bytes[4..8].try_into().unwrap())
        };
        return Ok(cpu == CPU_TYPE_ARM64);
    }

    let (fat64, big_endian) = match (magic_be, magic_le) {
        (FAT_MAGIC, _) => (false, true),
        (FAT_MAGIC_64, _) => (true, true),
        (_, FAT_MAGIC) => (false, false),
        (_, FAT_MAGIC_64) => (true, false),
        _ => {
            return Err(DiscoveryError::new(
                "CODEX_EXECUTABLE_FORMAT_UNSUPPORTED",
                "Executable is not a supported Mach-O file",
            ));
        }
    };
    let read_u32 = |slice: &[u8]| {
        if big_endian {
            u32::from_be_bytes(slice.try_into().unwrap())
        } else {
            u32::from_le_bytes(slice.try_into().unwrap())
        }
    };
    let count = usize::try_from(read_u32(&bytes[4..8])).map_err(|_| {
        DiscoveryError::new(
            "CODEX_EXECUTABLE_FORMAT_UNSUPPORTED",
            "Mach-O architecture count is invalid",
        )
    })?;
    let entry_size = if fat64 { 32 } else { 20 };
    let required = 8usize
        .checked_add(count.checked_mul(entry_size).ok_or_else(|| {
            DiscoveryError::new(
                "CODEX_EXECUTABLE_FORMAT_UNSUPPORTED",
                "Mach-O architecture table is too large",
            )
        })?)
        .filter(|required| *required <= MAX_MACHO_HEADER && *required <= bytes.len())
        .ok_or_else(|| {
            DiscoveryError::new(
                "CODEX_EXECUTABLE_FORMAT_UNSUPPORTED",
                "Mach-O architecture table is truncated",
            )
        })?;
    let _ = required;
    Ok((0..count).any(|index| {
        let offset = 8 + index * entry_size;
        read_u32(&bytes[offset..offset + 4]) == CPU_TYPE_ARM64
    }))
}

/// 在任何 version/schema probe 前完成 regular、execute bit 与 ARM64 Mach-O 检查。
pub(crate) fn preflight(path: &Path) -> Result<(), DiscoveryError> {
    let metadata = fs::metadata(path)
        .map_err(|error| DiscoveryError::new("CODEX_EXECUTABLE_NOT_RUNNABLE", error.to_string()))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(DiscoveryError::new(
            "CODEX_EXECUTABLE_NOT_RUNNABLE",
            "Candidate must be a regular executable file",
        ));
    }
    if macho_contains_arm64(path)? {
        Ok(())
    } else {
        Err(DiscoveryError::new(
            "CODEX_ARCH_UNSUPPORTED",
            "Candidate does not contain an ARM64 Mach-O slice",
        ))
    }
}

/// 逐个验证 canonical candidate；较早不兼容不会阻断后续候选。
pub(crate) async fn select_compatible<F, Fut>(
    candidates: Vec<PathBuf>,
    mut verify: F,
) -> Result<PathBuf, DiscoveryError>
where
    F: FnMut(PathBuf) -> Fut,
    Fut: Future<Output = Result<(), DiscoveryError>>,
{
    let mut canonical_seen = HashSet::new();
    let mut compatibility_failures = BTreeMap::new();
    let mut preflight_failures = BTreeMap::new();
    for candidate in candidates {
        let Ok(canonical) = fs::canonicalize(candidate) else {
            continue;
        };
        if !canonical.is_absolute() || !canonical_seen.insert(canonical.clone()) {
            continue;
        }
        if let Err(error) = preflight(&canonical) {
            let count = preflight_failures.entry(error.code).or_insert(0usize);
            *count = count.saturating_add(1);
            continue;
        }
        match verify(canonical.clone()).await {
            Ok(()) => return Ok(canonical),
            Err(error) => {
                let count = compatibility_failures.entry(error.code).or_insert(0usize);
                *count = count.saturating_add(1);
            }
        }
    }
    if !compatibility_failures.is_empty() {
        Err(DiscoveryError::new(
            "CODEX_COMPATIBILITY_BLOCKED",
            format!(
                "Runnable ARM64 candidates failed exact compatibility: {}",
                failure_summary(&compatibility_failures)
            ),
        ))
    } else {
        Err(DiscoveryError::new(
            "BACKEND_UNAVAILABLE",
            format!(
                "No runnable ARM64 Codex candidate; rejected={}",
                failure_summary(&preflight_failures)
            ),
        ))
    }
}

/// 正式 probe selection 的内部失败；只有 Runtime 类别携带 live ownership。
#[derive(Debug)]
pub(crate) enum SelectionFailure {
    Discovery(DiscoveryError),
    Runtime(ProbeRuntimeFailure),
}

/// 使用 typed probe 结果选择候选；Runtime cleanup 失败必须立即停止遍历。
pub(crate) async fn select_owned<F, Fut>(
    candidates: Vec<PathBuf>,
    mut verify: F,
) -> Result<PathBuf, SelectionFailure>
where
    F: FnMut(PathBuf) -> Fut,
    Fut: Future<Output = Result<(), ProbeFailure>>,
{
    let mut canonical_seen = HashSet::new();
    let mut compatibility_failures = BTreeMap::new();
    let mut preflight_failures = BTreeMap::new();
    for candidate in candidates {
        let Ok(canonical) = fs::canonicalize(candidate) else {
            continue;
        };
        if !canonical.is_absolute() || !canonical_seen.insert(canonical.clone()) {
            continue;
        }
        if let Err(error) = preflight(&canonical) {
            let count = preflight_failures.entry(error.code).or_insert(0usize);
            *count = count.saturating_add(1);
            continue;
        }
        match verify(canonical.clone()).await {
            Ok(()) => return Ok(canonical),
            Err(ProbeFailure::Compatibility(error)) => {
                let count = compatibility_failures.entry(error.code).or_insert(0usize);
                *count = count.saturating_add(1);
            }
            Err(ProbeFailure::Runtime(failure)) => {
                return Err(SelectionFailure::Runtime(failure));
            }
        }
    }
    let error = if !compatibility_failures.is_empty() {
        DiscoveryError::new(
            "CODEX_COMPATIBILITY_BLOCKED",
            format!(
                "Runnable ARM64 candidates failed exact compatibility: {}",
                failure_summary(&compatibility_failures)
            ),
        )
    } else {
        DiscoveryError::new(
            "BACKEND_UNAVAILABLE",
            format!(
                "No runnable ARM64 Codex candidate; rejected={}",
                failure_summary(&preflight_failures)
            ),
        )
    };
    Err(SelectionFailure::Discovery(error))
}

/// 生产 discovery 入口；兼容性 verifier 在 managed adapter 接入后提供。
pub(crate) async fn discover_with<F, Fut>(verify: F) -> Result<PathBuf, String>
where
    F: FnMut(PathBuf) -> Fut,
    Fut: Future<Output = Result<(), DiscoveryError>>,
{
    if !host_supported(std::env::consts::OS, std::env::consts::ARCH) {
        return Err(DiscoveryError::new(
            "CODEX_HOST_ARCH_UNSUPPORTED",
            "SerenaDesktop macOS first release requires native Apple Silicon",
        )
        .to_string());
    }
    select_compatible(enumerate_candidates(&DiscoveryContext::current()), verify)
        .await
        .map_err(|error| error.to_string())
}

/// 将 probe cleanup failure 交给既有 Pool 全局 quarantine，并返回无 owner 的诊断副本。
pub(crate) fn retain_probe_failure(
    context: &ProbeContext,
    failure: ProbeRuntimeFailure,
) -> super::runtime_adapter::RuntimeFailure {
    context.runtime_pool().retain_failure(
        &context.store(),
        "",
        &failure.runtime_id,
        failure.failure,
    )
}

/// 生产入口把每个 ARM64 Mach-O 交给同一正式 ownership domain 的 Contract verifier。
pub async fn discover(context: ProbeContext) -> Result<PathBuf, String> {
    if !host_supported(std::env::consts::OS, std::env::consts::ARCH) {
        return Err(DiscoveryError::new(
            "CODEX_HOST_ARCH_UNSUPPORTED",
            "SerenaDesktop macOS first release requires native Apple Silicon",
        )
        .to_string());
    }
    let probe_context = context.clone();
    match select_owned(
        enumerate_candidates(&DiscoveryContext::current()),
        move |path| {
            let probe_context = probe_context.clone();
            async move {
                super::app_server::managed::verify(&probe_context, path)
                    .await
                    .map(|_| ())
            }
        },
    )
    .await
    {
        Ok(path) => Ok(path),
        Err(SelectionFailure::Discovery(error)) => Err(error.to_string()),
        Err(SelectionFailure::Runtime(failure)) => {
            let diagnostic = retain_probe_failure(&context, failure);
            Err(format!("{}: {}", diagnostic.code, diagnostic.message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::codex::runtime_adapter::{RuntimeError, RuntimeFailure};
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };

    /// 写入最小 thin Mach-O header，并控制 execute bit。
    fn thin(path: &Path, cpu: u32, executable: bool) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xfeedfacfu32.to_le_bytes());
        bytes.extend_from_slice(&cpu.to_le_bytes());
        bytes.resize(32, 0);
        fs::write(path, bytes).unwrap();
        let mode = if executable { 0o755 } else { 0o644 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    /// 写入只包含指定 CPU types 的最小 fat Mach-O header。
    fn fat(path: &Path, cpus: &[u32]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xcafebabeu32.to_be_bytes());
        bytes.extend_from_slice(&(cpus.len() as u32).to_be_bytes());
        for cpu in cpus {
            bytes.extend_from_slice(&cpu.to_be_bytes());
            bytes.resize(bytes.len() + 16, 0);
        }
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// 测试上下文不读取当前进程环境，确保来源顺序可重复。
    fn context(root: &Path) -> DiscoveryContext {
        DiscoveryContext {
            path_entries: vec![root.join("PATH bin")],
            home: Some(root.join("用户 Home")),
            fixed_prefixes: vec![root.join("opt/homebrew"), root.join("usr/local")],
            chatgpt_candidate: root.join("Applications/ChatGPT.app/Contents/Resources/codex"),
        }
    }

    /// Runtime cleanup failure 是立即停止的 typed failure，不得继续验证后续候选。
    #[tokio::test]
    async fn probe_ownership_runtime_failure_stops_selection() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        thin(&first, CPU_TYPE_ARM64, true);
        thin(&second, CPU_TYPE_ARM64, true);
        let calls = Arc::new(Mutex::new(0usize));
        let observed = calls.clone();
        let result = select_owned(vec![first, second], move |_| {
            *observed.lock().unwrap() += 1;
            async {
                Err(ProbeFailure::Runtime(ProbeRuntimeFailure {
                    runtime_id: "probe-runtime".into(),
                    failure: RuntimeFailure::from(RuntimeError::new(
                        "CODEX_RUNTIME_TERMINATION_UNCONFIRMED",
                        "fixture",
                    )),
                }))
            }
        })
        .await;
        assert!(matches!(result, Err(SelectionFailure::Runtime(_))));
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    /// PATH 的错误版本不能阻断后续 npm ARM64 vendor binary。
    #[tokio::test]
    async fn incompatible_earlier_candidate_continues_to_vendor() {
        let directory = tempfile::tempdir().unwrap();
        let context = context(directory.path());
        let path_candidate = context.path_entries[0].join("codex");
        let vendor = context.fixed_prefixes[0].join(
            "lib/node_modules/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex",
        );
        thin(&path_candidate, CPU_TYPE_ARM64, true);
        thin(&vendor, CPU_TYPE_ARM64, true);

        let expected = vendor.canonicalize().unwrap();
        let selected = select_compatible(enumerate_candidates(&context), |path| {
            let compatible = path == expected;
            async move {
                if compatible {
                    Ok(())
                } else {
                    Err(DiscoveryError::new(
                        "CODEX_APP_SERVER_INCOMPATIBLE",
                        "fixture mismatch",
                    ))
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(selected, expected);
    }

    /// canonical symlink 与真实路径必须只验证一次，路径字符不影响选择。
    #[tokio::test]
    async fn canonical_dedupe_handles_symlink_spaces_and_chinese() {
        let directory = tempfile::tempdir().unwrap();
        let mut context = context(directory.path());
        let target = directory.path().join("Volumes/外置 磁盘/Codex 工具/codex");
        thin(&target, CPU_TYPE_ARM64, true);
        fs::create_dir_all(&context.path_entries[0]).unwrap();
        symlink(&target, context.path_entries[0].join("codex")).unwrap();
        context.home = Some(directory.path().join("Volumes/外置 磁盘/Codex 工具"));
        let local = context.home.as_ref().unwrap().join(".local/bin");
        fs::create_dir_all(&local).unwrap();
        symlink(&target, local.join("codex")).unwrap();

        let seen = Arc::new(Mutex::new(Vec::new()));
        let verifier_seen = seen.clone();
        let selected = select_compatible(enumerate_candidates(&context), move |path| {
            verifier_seen.lock().unwrap().push(path.clone());
            async { Ok(()) }
        })
        .await
        .unwrap();
        assert_eq!(selected, target.canonicalize().unwrap());
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    /// ChatGPT.app 永远是最后候选，缺失只表示该候选不可用。
    #[test]
    fn chatgpt_is_last_best_effort_candidate() {
        let directory = tempfile::tempdir().unwrap();
        let context = context(directory.path());
        let candidates = enumerate_candidates(&context);
        assert_eq!(candidates.last(), Some(&context.chatgpt_candidate));
    }

    /// PATH、用户目录、固定 Homebrew 路径、npm vendor 与 ChatGPT 必须保持冻结顺序。
    #[test]
    fn candidate_sources_keep_the_frozen_order() {
        let directory = tempfile::tempdir().unwrap();
        let context = context(directory.path());
        let candidates = enumerate_candidates(&context);
        let path = context.path_entries[0].join("codex");
        let local = context.home.as_ref().unwrap().join(".local/bin/codex");
        let homebrew = context.fixed_prefixes[0].join("bin/codex");
        let vendor = context.fixed_prefixes[0].join(
            "lib/node_modules/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex",
        );
        let position = |candidate: &Path| {
            candidates
                .iter()
                .position(|item| item == candidate)
                .expect("冻结来源必须存在")
        };
        assert!(position(&path) < position(&local));
        assert!(position(&local) < position(&homebrew));
        assert!(position(&homebrew) < position(&vendor));
        assert!(position(&vendor) < position(&context.chatgpt_candidate));
    }

    /// Finder 风格空 PATH 仍必须从固定位置选择兼容 ARM64 candidate。
    #[tokio::test]
    async fn finder_minimal_path_uses_fixed_candidate() {
        let directory = tempfile::tempdir().unwrap();
        let mut context = context(directory.path());
        context.path_entries.clear();
        let homebrew = context.fixed_prefixes[0].join("bin/codex");
        thin(&homebrew, CPU_TYPE_ARM64, true);
        let expected = homebrew.canonicalize().unwrap();
        let selected = select_compatible(enumerate_candidates(&context), |_| async { Ok(()) })
            .await
            .unwrap();
        assert_eq!(selected, expected);
    }

    /// 只有原生 macOS aarch64 Host 可进入首版 discovery。
    #[test]
    fn host_gate_rejects_intel_and_non_macos_targets() {
        assert!(host_supported("macos", "aarch64"));
        assert!(!host_supported("macos", "x86_64"));
        assert!(!host_supported("windows", "aarch64"));
    }

    /// thin/fat ARM64 可进入 compatibility，纯 x86_64 必须被拒绝。
    #[test]
    fn macho_architecture_matrix_is_arm64_only() {
        let directory = tempfile::tempdir().unwrap();
        let arm64 = directory.path().join("arm64");
        let x86 = directory.path().join("x86");
        let universal = directory.path().join("universal");
        let fat_x86 = directory.path().join("fat-x86");
        thin(&arm64, CPU_TYPE_ARM64, true);
        thin(&x86, CPU_TYPE_X86_64, true);
        fat(&universal, &[CPU_TYPE_X86_64, CPU_TYPE_ARM64]);
        fat(&fat_x86, &[CPU_TYPE_X86_64]);

        assert!(preflight(&arm64).is_ok());
        assert!(preflight(&universal).is_ok());
        for path in [&x86, &fat_x86] {
            assert_eq!(preflight(path).unwrap_err().code, "CODEX_ARCH_UNSUPPORTED");
        }
    }

    /// 非 Mach-O、截断 header 与无 execute bit 使用稳定拒绝类别。
    #[test]
    fn executable_format_and_permission_rejections_are_stable() {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("script");
        let javascript = directory.path().join("javascript");
        let truncated = directory.path().join("truncated");
        let no_execute = directory.path().join("no-execute");
        fs::write(&script, b"#!/bin/sh\nexec codex \"$@\"\n").unwrap();
        fs::write(&javascript, b"#!/usr/bin/env node\nrequire('codex')\n").unwrap();
        fs::write(&truncated, 0xfeedfacfu32.to_le_bytes()).unwrap();
        for path in [&script, &javascript, &truncated] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(
                preflight(path).unwrap_err().code,
                "CODEX_EXECUTABLE_FORMAT_UNSUPPORTED"
            );
        }
        thin(&no_execute, CPU_TYPE_ARM64, false);
        assert_eq!(
            preflight(&no_execute).unwrap_err().code,
            "CODEX_EXECUTABLE_NOT_RUNNABLE"
        );
    }

    /// 全部 ARM64 候选不兼容与完全无可运行候选必须有不同顶层诊断。
    #[tokio::test]
    async fn selection_distinguishes_blocked_from_unavailable() {
        let directory = tempfile::tempdir().unwrap();
        let arm64 = directory.path().join("codex");
        thin(&arm64, CPU_TYPE_ARM64, true);
        let blocked = select_compatible(vec![arm64], |_| async {
            Err(DiscoveryError::new(
                "CODEX_APP_SERVER_INCOMPATIBLE",
                "fixture mismatch",
            ))
        })
        .await
        .unwrap_err();
        assert_eq!(blocked.code, "CODEX_COMPATIBILITY_BLOCKED");
        assert!(blocked.message.contains("CODEX_APP_SERVER_INCOMPATIBLE=1"));

        let unavailable =
            select_compatible(vec![PathBuf::from("/missing/codex")], |_| async { Ok(()) })
                .await
                .unwrap_err();
        assert_eq!(unavailable.code, "BACKEND_UNAVAILABLE");
    }

    /// 顶层 unavailable 诊断必须保留候选拒绝类别，但不能泄露候选路径。
    #[tokio::test]
    async fn unavailable_summary_keeps_bounded_rejection_categories() {
        let directory = tempfile::tempdir().unwrap();
        let x86 = directory.path().join("私密 x86 路径/codex");
        let script = directory.path().join("私密 script 路径/codex");
        thin(&x86, CPU_TYPE_X86_64, true);
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        fs::write(&script, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

        let error = select_compatible(vec![x86, script], |_| async { Ok(()) })
            .await
            .unwrap_err();
        assert_eq!(error.code, "BACKEND_UNAVAILABLE");
        assert!(error.message.contains("CODEX_ARCH_UNSUPPORTED=1"));
        assert!(
            error
                .message
                .contains("CODEX_EXECUTABLE_FORMAT_UNSUPPORTED=1")
        );
        assert!(!error.message.contains("私密"));
    }
}
