use super::*;

use std::{
    io::{Read, Write},
    os::windows::process::CommandExt,
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
};

use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;

static IDS: AtomicU64 = AtomicU64::new(1);

/// 生成不会与并行测试冲突的 Runtime id。
fn runtime_id() -> String {
    format!(
        "fixture-{}-{}",
        std::process::id(),
        IDS.fetch_add(1, AtomicOrdering::Relaxed)
    )
}

/// 用可注入环境构造真实 launcher 请求。
fn request(executable: &Path, args: Vec<OsString>) -> LaunchRequest {
    let frozen = canonicalize_workspace_root(executable.parent().unwrap()).unwrap();
    let current_dir =
        ExternalProcessPath::verify(&frozen, UncCurrentDirectoryPolicy::Supported).unwrap();
    let mut baseline: Vec<_> = std::env::vars_os().collect();
    baseline.retain(|(name, _)| !os_eq_ignore_case(name, OsStr::new("CB6_HOST_UNICODE")));
    baseline.push((
        OsString::from("CB6_HOST_UNICODE"),
        OsString::from("继承-雪"),
    ));
    let environment = ProviderChildEnvironment::from_entries(
        baseline,
        &[executable.parent().unwrap().to_owned()],
    )
    .unwrap();
    LaunchRequest {
        executable: executable.to_owned(),
        args,
        current_dir,
        environment,
        runtime_instance_id: runtime_id(),
    }
}

/// 返回当前进程的 kernel handle 数。
fn handle_count() -> u32 {
    let mut count = 0;
    assert_ne!(
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) },
        0
    );
    count
}

/// 查询 handle inheritance flag。
fn handle_flags(handle: HANDLE) -> u32 {
    let mut flags = 0;
    assert_ne!(unsafe { GetHandleInformation(handle, &mut flags) }, 0);
    flags
}

/// 查询 Job 的 limit flags。
fn job_flags(job: HANDLE) -> u32 {
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    assert_ne!(
        unsafe {
            QueryInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&mut info as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                null_mut(),
            )
        },
        0
    );
    info.BasicLimitInformation.LimitFlags
}

/// 关闭 stdin、等待 child，并读取分离的 stdout/stderr。
fn finish(child: CreatedChild) -> (String, String) {
    let CreatedChild {
        stdin,
        mut stdout,
        mut stderr,
        process,
        job,
        ..
    } = child;
    drop(stdin);
    assert_eq!(
        unsafe { WaitForSingleObject(process.as_raw_handle(), 10_000) },
        WAIT_OBJECT_0,
        "child 未在 stdin EOF 后退出"
    );
    let mut output = String::new();
    let mut error = String::new();
    stdout.read_to_string(&mut output).unwrap();
    stderr.read_to_string(&mut error).unwrap();
    drop((stdout, stderr, process, job));
    (output, error)
}

/// 将 Unicode environment block 解码为测试字符串。
fn environment_strings(environment: &ProviderChildEnvironment) -> Vec<OsString> {
    assert!(environment.block.ends_with(&[0, 0]));
    environment.block[..environment.block.len() - 1]
        .split(|unit| *unit == 0)
        .filter(|entry| !entry.is_empty())
        .map(OsString::from_wide)
        .collect()
}

#[test]
fn quotes_absolute_unicode_argv_without_shell() {
    let cases = [
        ("", r#""""#),
        ("plain", r#""plain""#),
        ("a b", r#""a b""#),
        ("a\"b", r#""a\"b""#),
        ("a\\", r#""a\\""#),
        ("雪🦀", r#""雪🦀""#),
    ];
    for (argument, expected) in cases {
        let actual = command_line(OsStr::new(argument), &[]).unwrap();
        assert_eq!(
            String::from_utf16(&actual[..actual.len() - 1]).unwrap(),
            expected
        );
    }
    assert!(command_line(OsStr::new("a\0b"), &[]).is_err());
}

#[test]
fn external_path_projects_local_and_unc_without_changing_authority() {
    let directory = tempfile::tempdir().unwrap();
    let frozen = canonicalize_workspace_root(directory.path()).unwrap();
    let projected =
        ExternalProcessPath::verify(&frozen, UncCurrentDirectoryPolicy::Supported).unwrap();
    let projected_wide = projected
        .as_path()
        .as_os_str()
        .encode_wide()
        .collect::<Vec<_>>();
    assert!(!projected_wide.starts_with(&wide_ascii(r"\\?\")));
    verify_projected_identity(&frozen, projected.as_path()).unwrap();
    assert_eq!(
        project_external_path(projected.as_path(), UncCurrentDirectoryPolicy::Supported).unwrap(),
        projected.as_path()
    );

    let verbatim_unc = Path::new(r"\\?\UNC\server\share\workspace");
    assert_eq!(
        project_external_path(verbatim_unc, UncCurrentDirectoryPolicy::Supported).unwrap(),
        Path::new(r"\\server\share\workspace")
    );
    assert_eq!(
        project_external_path(
            Path::new(r"\\server\share\workspace"),
            UncCurrentDirectoryPolicy::Supported,
        )
        .unwrap(),
        Path::new(r"\\server\share\workspace")
    );
    assert_eq!(
        project_external_path(verbatim_unc, UncCurrentDirectoryPolicy::Unsupported)
            .unwrap_err()
            .code,
        UNSUPPORTED_UNC_CWD
    );
}

#[test]
fn external_path_rejects_identity_mismatch_and_missing_directory() {
    let frozen_directory = tempfile::tempdir().unwrap();
    let other_directory = tempfile::tempdir().unwrap();
    let frozen = canonicalize_workspace_root(frozen_directory.path()).unwrap();
    assert_eq!(
        verify_projected_identity(&frozen, other_directory.path())
            .unwrap_err()
            .code,
        INVALID_WORKSPACE_PATH
    );
    assert_eq!(
        verify_projected_identity(&frozen, &other_directory.path().join("missing"))
            .unwrap_err()
            .code,
        INVALID_WORKSPACE_PATH
    );
}

/// Node 主脚本只在外部进程边界投影 verbatim 本地盘符，普通路径保持不变。
#[test]
fn node_script_projects_verbatim_local_drive_and_keeps_ordinary_path() {
    let directory = tempfile::tempdir().unwrap();
    let ordinary = directory.path().join("codebuddy-script");
    std::fs::write(&ordinary, "fixture").unwrap();
    let canonical = std::fs::canonicalize(&ordinary).unwrap();

    assert!(
        canonical
            .as_os_str()
            .encode_wide()
            .collect::<Vec<_>>()
            .starts_with(&wide_ascii(r"\\?\"))
    );
    let projected = project_external_script_path(&canonical).unwrap();
    assert!(
        !projected
            .as_os_str()
            .encode_wide()
            .collect::<Vec<_>>()
            .starts_with(&wide_ascii(r"\\?\"))
    );
    verify_projected_file_identity(&canonical, &projected).unwrap();
    verify_projected_file_identity(&ordinary, &projected).unwrap();
}

/// Node 主脚本投影必须复验文件 identity，并拒绝所有越界或不可验证路径。
#[test]
fn node_script_revalidates_identity_and_rejects_invalid_paths() {
    let directory = tempfile::tempdir().unwrap();
    let frozen = directory.path().join("frozen-script");
    let other = directory.path().join("other-script");
    std::fs::write(&frozen, "frozen").unwrap();
    std::fs::write(&other, "other").unwrap();

    assert_eq!(
        verify_projected_file_identity(&frozen, &other)
            .unwrap_err()
            .code,
        INVALID_SCRIPT_PATH
    );
    for invalid in [
        PathBuf::from("relative-script"),
        PathBuf::from(r"\\server\share\codebuddy"),
        PathBuf::from(r"\\?\UNC\server\share\codebuddy"),
        PathBuf::from(r"\\?\Volume{fixture}\codebuddy"),
        directory.path().join("missing-script"),
        directory.path().to_owned(),
    ] {
        assert_eq!(
            project_external_script_path(&invalid).unwrap_err().code,
            INVALID_SCRIPT_PATH,
            "{invalid:?}"
        );
    }
}

#[test]
fn provider_environment_inherits_host_replaces_path_and_redacts_debug() {
    let entries = vec![
        (OsString::from("ZED"), OsString::from("last")),
        (OsString::from("path"), OsString::from("stale-secret-path")),
        (OsString::from("PaTh"), OsString::from("second-stale-path")),
        (OsString::from("TOKEN"), OsString::from("credential-secret")),
        (OsString::from("UNICODE_雪"), OsString::from("值🦀")),
    ];
    let projection = [
        PathBuf::from(r"C:\fresh one"),
        PathBuf::from(r"D:\fresh-two"),
    ];
    let environment = ProviderChildEnvironment::from_entries(entries, &projection).unwrap();
    let strings = environment_strings(&environment);
    assert!(strings.contains(&OsString::from("TOKEN=credential-secret")));
    assert!(strings.contains(&OsString::from("UNICODE_雪=值🦀")));
    assert_eq!(
        strings
            .iter()
            .filter(|entry| {
                let entry: Vec<_> = entry.encode_wide().collect();
                let Some(separator) = entry.iter().position(|unit| *unit == u16::from(b'=')) else {
                    return false;
                };
                os_eq_ignore_case(
                    &OsString::from_wide(&entry[..separator]),
                    OsStr::new("PATH"),
                )
            })
            .count(),
        1
    );
    assert!(strings.contains(&OsString::from(r"Path=C:\fresh one;D:\fresh-two")));
    let diagnostic = format!("{environment:?}");
    for secret in [
        "credential-secret",
        "stale-secret-path",
        "fresh one",
        "UNICODE_雪",
    ] {
        assert!(!diagnostic.contains(secret));
    }
    assert!(diagnostic.contains("<redacted>"));
}

#[test]
fn provider_environment_rejects_embedded_nul_and_has_double_nul_terminator() {
    let environment = ProviderChildEnvironment::from_entries(
        vec![
            (OsString::from("HOST"), OsString::from("继承")),
            (
                OsString::from("=C:"),
                OsString::from(r"C:\host-current-directory"),
            ),
        ],
        &[],
    )
    .unwrap();
    assert!(environment.block.ends_with(&[0, 0]));
    assert!(
        environment_strings(&environment)
            .contains(&OsString::from(r"=C:=C:\host-current-directory"))
    );
    assert!(
        ProviderChildEnvironment::from_entries(
            vec![(OsString::from("BAD\0NAME"), OsString::from("value"))],
            &[],
        )
        .is_err()
    );
    assert!(
        ProviderChildEnvironment::from_entries(
            vec![(OsString::from("NAME"), OsString::from("bad\0value"))],
            &[],
        )
        .is_err()
    );
    assert!(
        ProviderChildEnvironment::from_entries(
            vec![(OsString::from("BAD=NAME"), OsString::from("value"))],
            &[],
        )
        .is_err()
    );
}

#[test]
fn resolved_launch_spec_wires_without_starting_acp_and_rejects_wrappers() {
    let directory = tempfile::tempdir().unwrap();
    let frozen = canonicalize_workspace_root(directory.path()).unwrap();
    let executable = directory.path().join("node.exe");
    let script = directory
        .path()
        .join("node_modules")
        .join("pkg")
        .join("bin")
        .join("codebuddy");
    std::fs::create_dir_all(script.parent().unwrap()).unwrap();
    std::fs::write(&executable, "node fixture").unwrap();
    std::fs::write(&script, "script fixture").unwrap();
    let canonical_executable = std::fs::canonicalize(&executable).unwrap();
    let canonical_script = std::fs::canonicalize(&script).unwrap();
    let resolved = ResolvedLaunchSpec {
        executable: canonical_executable.clone(),
        args: vec![
            canonical_script.clone().into_os_string(),
            OsString::from("--acp"),
        ],
        path_projection: vec![directory.path().to_owned()],
    };
    let request = LaunchRequest::from_resolved(
        &resolved,
        &frozen,
        UncCurrentDirectoryPolicy::Supported,
        runtime_id(),
    )
    .unwrap();
    assert_eq!(request.executable, canonical_executable);
    assert_eq!(
        request.args[0],
        project_external_script_path(&canonical_script)
            .unwrap()
            .as_os_str()
    );
    assert_eq!(request.args[1], OsStr::new("--acp"));
    assert_eq!(resolved.args[0], canonical_script.as_os_str());
    assert_eq!(
        request.projected_cwd().as_path(),
        project_external_path(&frozen, UncCurrentDirectoryPolicy::Supported)
            .unwrap()
            .as_path()
    );
    assert!(
        environment_strings(&request.environment).contains(&OsString::from(format!(
            "Path={}",
            directory.path().display()
        )))
    );

    for extension in ["cmd", "bat", "ps1"] {
        let wrapper = ResolvedLaunchSpec {
            executable: directory.path().join(format!("codebuddy.{extension}")),
            args: vec![OsString::from("--acp")],
            path_projection: vec![],
        };
        assert_eq!(
            LaunchRequest::from_resolved(
                &wrapper,
                &frozen,
                UncCurrentDirectoryPolicy::Supported,
                runtime_id(),
            )
            .unwrap_err()
            .code,
            INVALID_INPUT
        );
    }

    let relative_script = ResolvedLaunchSpec {
        executable: directory.path().join("node.exe"),
        args: vec![
            OsString::from("relative-codebuddy"),
            OsString::from("--acp"),
        ],
        path_projection: vec![],
    };
    assert_eq!(
        LaunchRequest::from_resolved(
            &relative_script,
            &frozen,
            UncCurrentDirectoryPolicy::Supported,
            runtime_id(),
        )
        .unwrap_err()
        .code,
        INVALID_INPUT
    );

    let wrapper_script = ResolvedLaunchSpec {
        executable: directory.path().join("node.exe"),
        args: vec![
            directory.path().join("codebuddy.cmd").into_os_string(),
            OsString::from("--acp"),
        ],
        path_projection: vec![],
    };
    assert_eq!(
        LaunchRequest::from_resolved(
            &wrapper_script,
            &frozen,
            UncCurrentDirectoryPolicy::Supported,
            runtime_id(),
        )
        .unwrap_err()
        .code,
        INVALID_INPUT
    );
}

/// 在隔离测试进程内运行 handle-count 与真实 child-tree 证明。
#[test]
fn launcher_contract_with_real_child_tree() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("CodeBuddy 测试 Child.exe");
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codebuddy_launcher_child.rs");
    let build = std::process::Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "codebuddy_launcher_child"])
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "agent::codebuddy::windows_launcher::tests::isolated_launcher_contract",
            "--nocapture",
        ])
        .env("CB6_002_CHILD_EXE", &executable)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn isolated_launcher_contract() {
    let Some(executable) = std::env::var_os("CB6_002_CHILD_EXE") else {
        return;
    };
    let executable = PathBuf::from(executable);
    finish(launch(&request(&executable, vec![])).unwrap().child);
    let mut missing = request(&executable, vec![]);
    missing.executable = executable.with_file_name("missing.exe");
    assert_eq!(
        launch(&missing).unwrap_err().code,
        "CODEBUDDY_PROCESS_CREATE_FAILED"
    );
    let baseline = handle_count();

    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let raw_event = unsafe { CreateEventW(&security, 1, 0, null()) };
    assert!(!raw_event.is_null());
    let extra = unsafe { OwnedHandle::from_raw_handle(raw_event) };
    assert_ne!(handle_flags(extra.as_raw_handle()) & HANDLE_FLAG_INHERIT, 0);

    let mut launched = launch(&request(
        &executable,
        vec![
            OsString::from("--spawn-descendant"),
            executable.clone().into_os_string(),
        ],
    ))
    .unwrap();
    assert_eq!(handle_count(), baseline + 6);
    for handle in [
        launched.child.stdin.as_raw_handle(),
        launched.child.stdout.as_raw_handle(),
        launched.child.stderr.as_raw_handle(),
        launched.child.job.as_raw_handle(),
    ] {
        assert_eq!(handle_flags(handle) & HANDLE_FLAG_INHERIT, 0);
    }
    assert_eq!(
        job_flags(launched.child.job.as_raw_handle()),
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    );
    validate_job_policy(launched.child.job.as_raw_handle()).unwrap();
    let mut in_designated_job = 0;
    assert_ne!(
        unsafe {
            IsProcessInJob(
                launched.child.process.as_raw_handle(),
                launched.child.job.as_raw_handle(),
                &mut in_designated_job,
            )
        },
        0
    );
    assert_eq!(in_designated_job, 1);
    writeln!(
        launched.child.stdin,
        "PROBE {} {}",
        launched.child.job.as_raw_handle() as usize,
        extra.as_raw_handle() as usize
    )
    .unwrap();
    writeln!(launched.child.stdin, "ENV CB6_HOST_UNICODE").unwrap();
    writeln!(launched.child.stdin, "roundtrip 中文").unwrap();
    let token = launched.process_start_token();
    let (output, error) = finish(launched.child);
    assert!(
        output.starts_with(&format!("READY:1:1:{token}\n")),
        "{output}"
    );
    assert!(output.contains("DESCENDANT:1:1\n"), "{output}");
    assert!(output.contains("PROBE:0:0\n"), "{output}");
    assert_eq!(
        unsafe { WaitForSingleObject(extra.as_raw_handle(), 0) },
        WAIT_TIMEOUT
    );
    assert!(output.contains(&format!(
        "ENV:CB6_HOST_UNICODE:{}\n",
        OsStr::new("继承-雪")
            .encode_wide()
            .map(|unit| format!("{unit:04x}"))
            .collect::<String>()
    )));
    assert!(output.contains("INPUT:roundtrip 中文\n"));
    assert!(output.ends_with("EOF\n"));
    assert_eq!(error, "STDERR:separate\n");
    drop(extra);
    assert_eq!(handle_count(), baseline);

    // 每个 process 前 checkpoint 都必须由 RAII 释放已取得资源。
    for checkpoint in [
        Checkpoint::JobCreated,
        Checkpoint::JobConfigured,
        Checkpoint::StdinCreated,
        Checkpoint::StdoutCreated,
        Checkpoint::StderrCreated,
        Checkpoint::AttributesInitialized,
        Checkpoint::JobAttributeSet,
        Checkpoint::HandleAttributeSet,
    ] {
        let error = launch_inner(&request(&executable, vec![]), Some(checkpoint)).unwrap_err();
        assert_eq!(error.code, "TEST_INJECTED_API_FAILURE");
        assert!(error.created.is_none());
        assert_eq!(handle_count(), baseline, "{checkpoint:?}");
    }

    // CreateProcessW 后的失败必须把完整 owner 返回 caller。
    let error = launch_inner(
        &request(&executable, vec![]),
        Some(Checkpoint::ProcessCreated),
    )
    .unwrap_err();
    assert_eq!(error.code, "CODEBUDDY_PROCESS_IDENTITY_FAILED");
    let child = *error.created.unwrap();
    assert_ne!(child.pid, 0);
    assert_eq!(handle_count(), baseline + 5);
    finish(child);
    assert_eq!(handle_count(), baseline);

    // TerminateJobObject 必须终止仍在等 stdin 的受管进程。
    let child = launch(&request(&executable, vec![])).unwrap().child;
    assert_ne!(
        unsafe { TerminateJobObject(child.job.as_raw_handle(), 77) },
        0
    );
    assert_eq!(
        unsafe { WaitForSingleObject(child.process.as_raw_handle(), 5_000) },
        WAIT_OBJECT_0
    );
    drop(child);
    assert_eq!(handle_count(), baseline);

    // 唯一 Job handle close 必须触发 KILL_ON_JOB_CLOSE。
    let CreatedChild {
        stdin,
        stdout,
        stderr,
        process,
        job,
        ..
    } = launch(&request(&executable, vec![])).unwrap().child;
    drop(job);
    assert_eq!(
        unsafe { WaitForSingleObject(process.as_raw_handle(), 5_000) },
        WAIT_OBJECT_0
    );
    drop((stdin, stdout, stderr, process));
    assert_eq!(handle_count(), baseline);

    // 未配置的 Job 必须被 policy validator fail-closed。
    let raw_job = unsafe { CreateJobObjectW(null(), null()) };
    assert!(!raw_job.is_null());
    let unconfigured = unsafe { OwnedHandle::from_raw_handle(raw_job) };
    assert_eq!(
        validate_job_policy(unconfigured.as_raw_handle())
            .unwrap_err()
            .code,
        "CODEBUDDY_JOB_POLICY_INVALID"
    );
    drop(unconfigured);
    assert_eq!(handle_count(), baseline);

    // 即使 limits 正确，可继承 Job handle 仍必须被 policy validator 拒绝。
    let inheritable_security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let raw_job = unsafe { CreateJobObjectW(&inheritable_security, null()) };
    assert!(!raw_job.is_null());
    let inheritable_job = unsafe { OwnedHandle::from_raw_handle(raw_job) };
    let mut valid_limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    valid_limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    assert_ne!(
        unsafe {
            SetInformationJobObject(
                inheritable_job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&valid_limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        },
        0
    );
    assert_ne!(
        handle_flags(inheritable_job.as_raw_handle()) & HANDLE_FLAG_INHERIT,
        0
    );
    assert_eq!(
        validate_job_policy(inheritable_job.as_raw_handle())
            .unwrap_err()
            .code,
        "CODEBUDDY_JOB_POLICY_INVALID"
    );
    drop(inheritable_job);
    assert_eq!(handle_count(), baseline);

    run_real_node_tree_if_available(&executable);
}

/// 若本机已有 node.exe，则证明真实 node root 与其 native descendant 同属 Job。
fn run_real_node_tree_if_available(fixture: &Path) {
    let Some(node) = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .map(|directory| directory.join("node.exe"))
        .find(|candidate| candidate.is_file())
    else {
        eprintln!("CB6_002_NODE_EVIDENCE_UNAVAILABLE: local node.exe not found");
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    let projected_script = directory.path().join("projected-argv.js");
    std::fs::write(&projected_script, "process.stdout.write(process.argv[1]);").unwrap();
    let canonical_node = std::fs::canonicalize(&node).unwrap();
    let canonical_script = std::fs::canonicalize(&projected_script).unwrap();
    let frozen = canonicalize_workspace_root(directory.path()).unwrap();
    let resolved = ResolvedLaunchSpec {
        executable: canonical_node,
        args: vec![canonical_script.into_os_string(), OsString::from("--acp")],
        path_projection: vec![node.parent().unwrap().to_owned()],
    };
    let projected_request = LaunchRequest::from_resolved(
        &resolved,
        &frozen,
        UncCurrentDirectoryPolicy::Supported,
        runtime_id(),
    )
    .unwrap();
    assert!(
        !projected_request.args[0]
            .encode_wide()
            .collect::<Vec<_>>()
            .starts_with(&wide_ascii(r"\\?\"))
    );
    let expected_projected_script = project_external_script_path(&canonical_script).unwrap();
    let (projected_argv, error) = finish(launch(&projected_request).unwrap().child);
    assert_eq!(projected_argv, expected_projected_script.to_string_lossy());
    assert!(error.is_empty(), "{error}");

    let script = directory.path().join("contained-tree.js");
    std::fs::write(
        &script,
        "const cp=require('child_process');const r=cp.spawnSync(process.argv[2],['--descendant'],{stdio:'inherit'});process.exit(r.status??1);",
    )
    .unwrap();
    let launched = launch(&request(
        &node,
        vec![
            script.into_os_string(),
            fixture.as_os_str().to_owned(),
            OsString::from("--acp"),
        ],
    ))
    .unwrap();
    let mut member = 0;
    assert_ne!(
        unsafe {
            IsProcessInJob(
                launched.child.process.as_raw_handle(),
                launched.child.job.as_raw_handle(),
                &mut member,
            )
        },
        0
    );
    assert_eq!(member, 1, "node.exe root 必须从创建时属于 Job");
    let (output, error) = finish(launched.child);
    assert!(output.contains("DESCENDANT:1:1\n"), "{output}\n{error}");
}
