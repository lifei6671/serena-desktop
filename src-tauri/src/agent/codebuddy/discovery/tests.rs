use super::*;

/// 把目录列表编码为当前平台 PATH 字符串，测试不修改进程环境。
fn path_value(paths: &[&Path]) -> OsString {
    std::env::join_paths(paths).unwrap()
}

/// 创建一个可读空文件；discovery 只验证读取，不执行 fixture。
fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, []).unwrap();
}

/// 创建典型 npm wrapper、真实 node 和已安装 CLI script。
fn npm_wrapper(root: &Path, package: Option<&str>) -> (PathBuf, PathBuf, PathBuf) {
    let wrapper = root.join("codebuddy.cmd");
    let node = root.join("node.exe");
    let script = root.join(PACKAGE_SCRIPT);
    touch(&node);
    touch(&script);
    fs::write(
        &wrapper,
        r#"@ECHO off
"%~dp0\node.exe" "%~dp0\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy" %*
"#,
    )
    .unwrap();
    if let Some(package) = package {
        fs::write(
            script
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("package.json"),
            package,
        )
        .unwrap();
    }
    (wrapper, node, script)
}

/// 构造完全受控的 resolver 输入，避免读取开发机环境或 Registry。
fn input() -> DiscoveryInput {
    DiscoveryInput::default()
}

/// 提取预期失败，避免要求成功结果实现可能泄露 PATH projection 的 `Debug`。
fn error(result: Result<DiscoveryResult, DiscoveryError>) -> DiscoveryError {
    match result {
        Err(error) => error,
        Ok(_) => panic!("discovery unexpectedly succeeded"),
    }
}

/// 完全缺失时返回稳定 unavailable 诊断，不泄露被扫描目录。
#[test]
fn missing_cli_is_fail_closed_and_redacted() {
    let directory = tempfile::tempdir().unwrap();
    let mut input = input();
    input.process_path = Some(path_value(&[directory.path()]));
    input.environment = vec![("PRIVATE_TOKEN".into(), "never-log-this".into())];
    let error = error(discover(input));
    assert_eq!(error, DiscoveryError::not_found(false));
    let diagnostic = format!("{error:?}");
    assert!(!diagnostic.contains(&directory.path().to_string_lossy().to_string()));
    assert!(!diagnostic.contains("never-log-this"));
}

/// direct CodeBuddy executable 解析为绝对 executable + `--acp`，metadata 可缺失。
#[test]
fn direct_codebuddy_executable_is_discovered_without_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("codebuddy.exe");
    touch(&executable);
    let mut input = input();
    input.process_path = Some(path_value(&[directory.path()]));
    let result = discover(input).unwrap();
    assert_eq!(
        result.launch_spec.executable,
        executable.canonicalize().unwrap()
    );
    assert_eq!(result.launch_spec.args, [OsString::from("--acp")]);
    assert_eq!(result.provenance.source, DiscoverySource::Process);
    assert!(!result.provenance.wrapper_resolved);
    assert_eq!(result.metadata.status, MetadataStatus::Missing);
}

/// IDE-only buddycn 只产生 hint，不能成为 CodeBuddy Provider executable。
#[test]
fn buddycn_only_is_never_registered_as_acp_executable() {
    let directory = tempfile::tempdir().unwrap();
    touch(&directory.path().join("buddycn.exe"));
    let mut input = input();
    input.process_path = Some(path_value(&[directory.path()]));
    assert_eq!(error(discover(input)), DiscoveryError::not_found(true));
}

/// npm metadata 缺失不阻断 wrapper discovery，也不伪造产品版本。
#[test]
fn missing_package_metadata_does_not_block_wrapper() {
    let directory = tempfile::tempdir().unwrap();
    npm_wrapper(directory.path(), None);
    let mut input = input();
    input.process_path = Some(path_value(&[directory.path()]));
    let result = discover(input).unwrap();
    assert_eq!(result.metadata, VersionMetadata::missing());
    assert_eq!(result.provenance.metadata_status, MetadataStatus::Missing);
}

/// malformed metadata 只形成诊断状态，真实 LaunchSpec 仍然可用。
#[test]
fn malformed_package_metadata_does_not_block_wrapper() {
    let directory = tempfile::tempdir().unwrap();
    npm_wrapper(directory.path(), Some("{not-json"));
    let mut input = input();
    input.process_path = Some(path_value(&[directory.path()]));
    let result = discover(input).unwrap();
    assert_eq!(result.metadata.status, MetadataStatus::Malformed);
    assert!(result.metadata.product_version.is_none());
    assert!(result.metadata.base_version.is_none());
    assert!(result.metadata.package_version.is_none());
}

/// 陈旧 process PATH miss 后，每次调用都能消费新的 HKCU Registry PATH。
#[test]
fn registry_path_refresh_recovers_stale_process_path() {
    let stale = tempfile::tempdir().unwrap();
    let current_user = tempfile::tempdir().unwrap();
    touch(&current_user.path().join("codebuddy.exe"));
    let mut input = input();
    input.process_path = Some(path_value(&[stale.path()]));
    input.registry_current_user_path = Some(path_value(&[current_user.path()]));
    let result = discover(input).unwrap();
    assert_eq!(
        result.provenance.source,
        DiscoverySource::RegistryCurrentUser
    );
}

/// explicit PATH 必须先于 process、HKCU、HKLM 和 safe common sources。
#[test]
fn source_precedence_keeps_first_resolved_candidate() {
    let explicit = tempfile::tempdir().unwrap();
    let process = tempfile::tempdir().unwrap();
    let current_user = tempfile::tempdir().unwrap();
    let local_machine = tempfile::tempdir().unwrap();
    let common = tempfile::tempdir().unwrap();
    for directory in [&explicit, &process, &current_user, &local_machine, &common] {
        touch(&directory.path().join("codebuddy.exe"));
    }
    let mut input = input();
    input.explicit_path = Some(path_value(&[explicit.path()]));
    input.process_path = Some(path_value(&[process.path()]));
    input.registry_current_user_path = Some(path_value(&[current_user.path()]));
    input.registry_local_machine_path = Some(path_value(&[local_machine.path()]));
    input.safe_common_dirs = vec![common.path().into()];
    let result = discover(input).unwrap();
    assert_eq!(result.provenance.source, DiscoverySource::Explicit);
    assert_eq!(
        result.launch_spec.executable,
        explicit
            .path()
            .join("codebuddy.exe")
            .canonicalize()
            .unwrap()
    );
}

/// crate-private explicit executable 在所有 PATH sources 之前解析，但不新增公共配置字段。
#[test]
fn explicit_executable_precedes_all_path_sources() {
    let explicit = tempfile::tempdir().unwrap();
    let process = tempfile::tempdir().unwrap();
    let explicit_executable = explicit.path().join("custom-codebuddy.exe");
    touch(&explicit_executable);
    touch(&process.path().join("codebuddy.exe"));
    let mut input = input();
    input.explicit_executable = Some(explicit_executable.clone());
    input.process_path = Some(path_value(&[process.path()]));
    let result = discover(input).unwrap();
    assert_eq!(result.provenance.source, DiscoverySource::Explicit);
    assert_eq!(
        result.launch_spec.executable,
        explicit_executable.canonicalize().unwrap()
    );
}

/// `%VAR%` 展开后按 Windows 大小写不敏感规则去重，保留首次来源。
#[test]
fn variables_expand_before_case_insensitive_dedupe() {
    let directory = tempfile::tempdir().unwrap();
    touch(&directory.path().join("codebuddy.exe"));
    let duplicate = PathBuf::from(directory.path().to_string_lossy().to_ascii_uppercase());
    let mut input = input();
    input.explicit_path = Some(path_value(&[Path::new("%TOOLS%"), &duplicate]));
    input.environment = vec![("tools".into(), directory.path().as_os_str().to_owned())];
    let result = discover(input).unwrap();
    assert_eq!(result.launch_spec.path_projection.len(), 1);
    assert_eq!(result.provenance.source, DiscoverySource::Explicit);
}

/// Windows ordinal ignore-case 覆盖非 ASCII 大小写，同时保留 drive root 与 drive-relative 差异。
#[cfg(windows)]
#[test]
fn path_dedupe_uses_windows_ordinal_identity_without_collapsing_drive_root() {
    let mut input = input();
    input.explicit_path = Some(path_value(&[
        Path::new(r"C:\ÄTools"),
        Path::new(r"c:\ätools"),
        Path::new(r"C:\"),
        Path::new("c:"),
    ]));
    let projected = project_paths(&input);
    assert_eq!(projected.len(), 3);
    assert_eq!(projected[0].path, PathBuf::from(r"C:\ÄTools"));
    assert_eq!(projected[1].path, PathBuf::from(r"C:\"));
    assert_eq!(projected[2].path, PathBuf::from("c:"));
}

/// `.ps1` 与无扩展 shim 即使可读也不能进入 executable 候选集。
#[test]
fn unsupported_extensions_and_unix_shims_are_ignored() {
    let directory = tempfile::tempdir().unwrap();
    touch(&directory.path().join("codebuddy.ps1"));
    touch(&directory.path().join("codebuddy"));
    let mut input = input();
    input.process_path = Some(path_value(&[directory.path()]));
    assert_eq!(error(discover(input)), DiscoveryError::not_found(false));
}

/// npm wrapper 最终解析为 node.exe + installed script + `--acp`，不执行 cmd/bat。
#[test]
fn npm_wrapper_resolves_real_executable_and_argv() {
    let directory = tempfile::tempdir().unwrap();
    let (wrapper, node, script) = npm_wrapper(
        directory.path(),
        Some(r#"{"version":"2.158.0","baseVersion":"1.106.1"}"#),
    );
    let mut input = input();
    input.process_path = Some(path_value(&[directory.path()]));
    let result = discover(input).unwrap();
    assert_ne!(
        result.launch_spec.executable,
        wrapper.canonicalize().unwrap()
    );
    assert_eq!(result.launch_spec.executable, node.canonicalize().unwrap());
    assert_eq!(
        result.launch_spec.args,
        [
            script.canonicalize().unwrap().into_os_string(),
            OsString::from("--acp")
        ]
    );
    assert!(result.provenance.wrapper_resolved);
    assert!(result.metadata.product_version.is_none());
    assert_eq!(result.metadata.base_version.as_deref(), Some("1.106.1"));
    assert_eq!(result.metadata.package_version.as_deref(), Some("2.158.0"));
    assert_eq!(result.metadata.status, MetadataStatus::Parsed);
}

/// base version 不能回填产品版本，二者始终保持独立诊断字段。
#[test]
fn base_version_never_becomes_product_version() {
    let directory = tempfile::tempdir().unwrap();
    npm_wrapper(directory.path(), Some(r#"{"baseVersion":"1.106.1"}"#));
    let mut input = input();
    input.process_path = Some(path_value(&[directory.path()]));
    let result = discover(input).unwrap();
    assert!(result.metadata.product_version.is_none());
    assert_eq!(result.metadata.base_version.as_deref(), Some("1.106.1"));
    assert!(result.metadata.package_version.is_none());
}

/// 安全 provenance 不包含完整 PATH、任意 env secret 或 wrapper 正文。
#[test]
fn diagnostic_provenance_is_minimal_and_redacted() {
    let directory = tempfile::tempdir().unwrap();
    npm_wrapper(directory.path(), None);
    let mut input = input();
    input.process_path = Some(path_value(&[directory.path()]));
    input.environment = vec![("TOKEN".into(), "diagnostic-secret".into())];
    let result = discover(input).unwrap();
    let diagnostic = format!("{:?}", result.provenance);
    assert!(!diagnostic.contains("diagnostic-secret"));
    assert!(!diagnostic.contains("node_modules/@tencent-ai"));
    assert!(!diagnostic.contains("@ECHO"));
}
