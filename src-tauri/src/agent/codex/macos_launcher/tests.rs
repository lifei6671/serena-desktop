use super::*;
use std::{
    ffi::OsString,
    io::{Read, Write},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

/// 构造仅覆盖待测字段的 launcher 请求。
fn request(executable: &Path, cwd: &Path) -> MacosLaunchRequest {
    MacosLaunchRequest {
        executable: executable.to_owned(),
        args: Vec::new(),
        current_dir: cwd.to_owned(),
        runtime_instance_id: "macos-runtime-fixture".into(),
    }
}

/// 直接使用 rustc 编译固定 fixture，避免测试经过 shell command string。
fn fixture(directory: &Path) -> PathBuf {
    let executable = directory.join("macos-runtime-child");
    let output = std::process::Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "macos_runtime_child"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/macos_runtime_child.rs"))
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

/// launcher 必须原样传递参数、目录和 stdio，并建立可重复观测的独立 Session。
#[test]
fn launch_preserves_argv_cwd_stdio_and_creates_verified_session() {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().join("工作 目录");
    std::fs::create_dir(&cwd).unwrap();
    let executable = fixture(directory.path());
    let mut request = request(&executable, &cwd);
    request.args = vec!["report".into(), "参数 值".into()];

    let launched = launch(&request).unwrap();
    assert_eq!(launched.identity.pid, launched.identity.pgid);
    assert_eq!(launched.identity.pid, launched.identity.sid);
    assert_eq!(launched.child.pid, launched.identity.pid);
    assert_eq!(launched.child.pgid, launched.identity.pgid);
    let repeated = MacosProcessIdentityAdapter::observe(launched.identity.pid).unwrap();
    assert_eq!(repeated, launched.identity);
    assert!(
        process_group_members(launched.identity.pgid)
            .unwrap()
            .contains(&launched.identity.pid)
    );

    let identity = launched.identity;
    // macOS 的临时目录可能经 /var 符号链接进入，child 的 getcwd 返回物理路径。
    let observed_cwd = cwd.canonicalize().unwrap();
    let mut child = launched.child;
    child.stdin.write_all("输入".as_bytes()).unwrap();
    drop(child.stdin);
    let mut stdout = String::new();
    let mut stderr = String::new();
    child.stdout.read_to_string(&mut stdout).unwrap();
    child.stderr.read_to_string(&mut stderr).unwrap();
    assert!(child.process.wait().unwrap().success());
    assert_eq!(
        stdout,
        format!(
            "pid={0};pgid={0};sid={0};cwd={1};arg=参数 值;stdin=输入\n",
            identity.pid,
            observed_cwd.display(),
        )
    );
    assert_eq!(stderr, "stderr-ready\n");
}

/// 任一身份字段变化都不得匹配创建时进程。
#[test]
fn identity_mismatch_never_matches_created_process() {
    let actual = ProcessIdentity {
        pid: 10,
        pgid: 10,
        sid: 10,
        start_token: ProcessStartToken {
            seconds: 20,
            microseconds: 30,
        },
    };

    let mut mismatched = actual.clone();
    mismatched.pid += 1;
    assert!(!actual.matches(&mismatched));

    mismatched = actual.clone();
    mismatched.pgid += 1;
    assert!(!actual.matches(&mismatched));

    mismatched = actual.clone();
    mismatched.sid += 1;
    assert!(!actual.matches(&mismatched));

    mismatched = actual.clone();
    mismatched.start_token.microseconds += 1;
    assert!(!actual.matches(&mismatched));
}

/// 版本化 start token 必须无损往返，且拒绝所有非契约格式。
#[test]
fn process_start_token_codec_is_versioned_and_strict() {
    let token = ProcessStartToken {
        seconds: 1_234,
        microseconds: 567_890,
    };
    let encoded = token.encode();
    assert_eq!(encoded, "darwin_proc_bsd_start_v1:1234:567890");
    assert_eq!(ProcessStartToken::decode(&encoded).unwrap(), token);

    for invalid in [
        "windows_filetime_v1:1234:567890",
        "darwin_proc_bsd_start_v1:1234",
        "darwin_proc_bsd_start_v1:1234:1:extra",
        "darwin_proc_bsd_start_v1:not-a-number:1",
        "darwin_proc_bsd_start_v1:-1:1",
        "darwin_proc_bsd_start_v1:1:-1",
        "darwin_proc_bsd_start_v1:1:1000000",
    ] {
        assert!(ProcessStartToken::decode(invalid).is_err(), "{invalid}");
    }
}

/// proc_pidinfo 返回零且未设置 errno 时必须解释为进程不存在。
#[test]
fn proc_pidinfo_read_zero_without_errno_is_esrch() {
    let error = classify_proc_pidinfo_read(0, 128, 0).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(libc::ESRCH));
}

/// proc_pidinfo 返回零且设置 errno 时必须保留系统错误。
#[test]
fn proc_pidinfo_read_zero_with_errno_preserves_system_error() {
    let error = classify_proc_pidinfo_read(0, 128, libc::EACCES).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(libc::EACCES));
}

/// proc_pidinfo 的正数短读是协议错误，不得伪装成进程不存在或其他 errno。
#[test]
fn proc_pidinfo_positive_short_read_is_eproto() {
    for errno in [0, libc::EACCES] {
        let error = classify_proc_pidinfo_read(64, 128, errno).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EPROTO));
    }
}

/// 合法 launcher 输入及精确长度上限必须通过校验。
#[test]
fn valid_launcher_inputs_accept_exact_limits() {
    let directory = tempfile::tempdir().unwrap();
    let executable = std::env::current_exe().unwrap();

    assert!(validate(&request(&executable, directory.path())).is_ok());

    let mut exact_runtime_id = request(&executable, directory.path());
    exact_runtime_id.runtime_instance_id = "r".repeat(MAX_RUNTIME_ID_BYTES);
    assert!(validate(&exact_runtime_id).is_ok());

    // 总长度包含 executable、一个参数以及两项各自的终止 NUL。
    let executable_bytes = executable.as_os_str().as_bytes().len();
    assert!(executable_bytes + 2 < MAX_COMMAND_BYTES);
    let exact_argument_bytes = MAX_COMMAND_BYTES - executable_bytes - 2;
    let mut exact_command = request(&executable, directory.path());
    exact_command
        .args
        .push(OsString::from_vec(vec![b'x'; exact_argument_bytes]));
    assert!(validate(&exact_command).is_ok());

    exact_command.args[0] = OsString::from_vec(vec![b'x'; exact_argument_bytes + 1]);
    assert_eq!(
        validate(&exact_command).unwrap_err().code,
        "CODEX_LAUNCH_INPUT_INVALID"
    );
}

/// 非法 launcher 输入必须在创建进程前被稳定拒绝。
#[test]
fn invalid_launcher_inputs_fail_before_spawn() {
    let directory = tempfile::tempdir().unwrap();
    let executable = std::env::current_exe().unwrap();

    let mut empty_id = request(&executable, directory.path());
    empty_id.runtime_instance_id.clear();
    assert_eq!(
        validate(&empty_id).unwrap_err().code,
        "CODEX_LAUNCH_INPUT_INVALID"
    );

    for separator in ["runtime/id", "runtime\\id"] {
        let mut separated_id = request(&executable, directory.path());
        separated_id.runtime_instance_id = separator.into();
        assert_eq!(
            validate(&separated_id).unwrap_err().code,
            "CODEX_LAUNCH_INPUT_INVALID"
        );
    }

    let mut nul_id = request(&executable, directory.path());
    nul_id.runtime_instance_id = "runtime\0id".into();
    assert_eq!(
        validate(&nul_id).unwrap_err().code,
        "CODEX_LAUNCH_INPUT_INVALID"
    );

    let mut oversized_id = request(&executable, directory.path());
    oversized_id.runtime_instance_id = "r".repeat(MAX_RUNTIME_ID_BYTES + 1);
    assert_eq!(
        validate(&oversized_id).unwrap_err().code,
        "CODEX_LAUNCH_INPUT_INVALID"
    );

    let relative_executable = request(Path::new("relative-bin"), directory.path());
    assert_eq!(
        validate(&relative_executable).unwrap_err().code,
        "CODEX_LAUNCH_INPUT_INVALID"
    );

    let relative_cwd = request(&executable, Path::new("relative-cwd"));
    assert_eq!(
        validate(&relative_cwd).unwrap_err().code,
        "CODEX_LAUNCH_INPUT_INVALID"
    );

    let missing_cwd = request(&executable, &directory.path().join("missing"));
    assert_eq!(
        validate(&missing_cwd).unwrap_err().code,
        "CODEX_LAUNCH_INPUT_INVALID"
    );

    let mut nul_argv = request(&executable, directory.path());
    nul_argv.args.push(OsString::from_vec(b"bad\0arg".to_vec()));
    assert_eq!(
        validate(&nul_argv).unwrap_err().code,
        "CODEX_LAUNCH_INPUT_INVALID"
    );

    let nul_executable_path = PathBuf::from(OsString::from_vec(b"/tmp/bad\0bin".to_vec()));
    let nul_executable = request(&nul_executable_path, directory.path());
    assert_eq!(
        validate(&nul_executable).unwrap_err().code,
        "CODEX_LAUNCH_INPUT_INVALID"
    );

    let nul_cwd_path = PathBuf::from(OsString::from_vec(b"/tmp/bad\0cwd".to_vec()));
    let nul_cwd = request(&executable, &nul_cwd_path);
    assert_eq!(
        validate(&nul_cwd).unwrap_err().code,
        "CODEX_LAUNCH_INPUT_INVALID"
    );

    let mut oversized_command = request(&executable, directory.path());
    oversized_command
        .args
        .push(OsString::from_vec(vec![b'x'; MAX_COMMAND_BYTES]));
    assert_eq!(
        validate(&oversized_command).unwrap_err().code,
        "CODEX_LAUNCH_INPUT_INVALID"
    );
}
