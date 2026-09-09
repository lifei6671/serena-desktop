use super::*;
use std::{
    io::{Read, Write},
    os::windows::process::CommandExt,
    sync::atomic::{AtomicU64, Ordering},
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;

static IDS: AtomicU64 = AtomicU64::new(1);
fn request(executable: &std::path::Path) -> LaunchRequest {
    LaunchRequest {
        executable: executable.to_owned(),
        current_dir: executable.parent().unwrap().to_owned(),
        args: vec![],
        runtime_instance_id: format!(
            "fixture-{}-{}",
            std::process::id(),
            IDS.fetch_add(1, Ordering::Relaxed)
        ),
    }
}
fn handle_count() -> u32 {
    let mut count = 0;
    assert_ne!(
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) },
        0
    );
    count
}
fn flags(handle: HANDLE) -> u32 {
    let mut flags = 0;
    assert_ne!(unsafe { GetHandleInformation(handle, &mut flags) }, 0);
    flags
}
fn policy(job: HANDLE) -> u32 {
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
        unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) },
        WAIT_OBJECT_0,
        "Child failed to observe stdin EOF"
    );
    let mut out = String::new();
    let mut err = String::new();
    stdout.read_to_string(&mut out).unwrap();
    stderr.read_to_string(&mut err).unwrap();
    drop((stdout, stderr, process, job));
    (out, err)
}

#[test]
fn quotes_windows_argv_without_loss() {
    let cases = [
        ("", r#""""#),
        ("plain", r#""plain""#),
        ("a b", r#""a b""#),
        ("a\"b", r#""a\"b""#),
        ("a\\", r#""a\\""#),
        ("雪🦀", r#""雪🦀""#),
    ];
    for (arg, expected) in cases {
        let actual = command_line(OsStr::new(arg), &[]).unwrap();
        assert_eq!(
            String::from_utf16(&actual[..actual.len() - 1]).unwrap(),
            expected
        );
    }
    assert!(command_line(OsStr::new("a\0b"), &[]).is_err());
}

/// Run in an isolated test process: handle-count assertions must not race the
/// rest of Desktop's tests or asynchronous runtime thread initialization.
#[test]
fn launcher_contract_with_real_child() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("测试 Child.exe");
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/launcher_child.rs");
    let build = std::process::Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "launcher_child"])
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
            "agent::codex::windows_launcher::tests::isolated_contract",
            "--nocapture",
        ])
        .env("TASK003_CHILD_EXE", &executable)
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
fn isolated_contract() {
    let Some(executable) = std::env::var_os("TASK003_CHILD_EXE") else {
        return;
    };
    let executable = PathBuf::from(executable);
    // Warm up stdio/system lazy initialization before exact handle accounting.
    finish(launch(&request(&executable)).unwrap().child);
    // The first failing CreateProcessW also initializes process-wide Windows
    // facilities. Include that path before measuring repeated resource ownership.
    let mut missing = request(&executable);
    missing.executable = executable.with_file_name("missing.exe");
    assert_eq!(
        launch(&missing).unwrap_err().code,
        "CODEX_PROCESS_CREATE_FAILED"
    );
    let baseline = handle_count();
    let mut req = request(&executable);
    req.args = [
        "",
        "plain",
        "has spaces",
        "a\"b",
        "before\\\"quote",
        "trailing\\",
        "two trailing\\\\",
        "雪 路径 🦀",
        "tab\there",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let raw = unsafe { CreateEventW(&security, 1, 0, null()) };
    assert!(!raw.is_null());
    let extra = unsafe { OwnedHandle::from_raw_handle(raw) };
    assert_ne!(flags(extra.as_raw_handle()) & HANDLE_FLAG_INHERIT, 0);
    let before = handle_count();
    let mut launched = launch(&req).unwrap();
    assert_eq!(
        handle_count(),
        before + 5,
        "only process, Job and three parent pipe handles remain"
    );
    for handle in [
        launched.child.stdin.as_raw_handle(),
        launched.child.stdout.as_raw_handle(),
        launched.child.stderr.as_raw_handle(),
        launched.child.job.as_raw_handle(),
    ] {
        assert_eq!(flags(handle) & HANDLE_FLAG_INHERIT, 0);
    }
    assert_eq!(
        policy(launched.child.job.as_raw_handle()),
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    );
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
    writeln!(launched.child.stdin, "roundtrip 中文").unwrap();
    let token = launched.process_start_token();
    assert_eq!(token.len(), 16);
    let (output, error) = finish(launched.child);
    assert!(
        output.starts_with(&format!("READY:1:1:{token}\n")),
        "{output}"
    );
    assert!(
        output.contains("PROBE:0:0\n"),
        "unexpected inherited Job/event: {output}"
    );
    assert_eq!(
        unsafe { WaitForSingleObject(extra.as_raw_handle(), 0) },
        WAIT_TIMEOUT
    );
    assert!(output.contains("INPUT:roundtrip 中文\n"));
    assert!(output.ends_with("EOF\n"));
    assert_eq!(error, "STDERR:separate\n");
    let actual: Vec<_> = output
        .lines()
        .filter_map(|line| line.strip_prefix("ARG:"))
        .map(str::to_owned)
        .collect();
    let expected: Vec<_> = std::iter::once(req.executable.as_os_str())
        .chain(req.args.iter().map(OsString::as_os_str))
        .map(|arg| {
            arg.encode_wide()
                .map(|v| format!("{v:04x}"))
                .collect::<String>()
        })
        .collect();
    assert_eq!(
        actual, expected,
        "argv[0] absolute Unicode exe and all quoting cases"
    );
    drop(extra);
    assert_eq!(handle_count(), baseline);

    // Same-name collision must leave the original Job limits untouched.
    let collision = request(&executable);
    let name = wide(OsStr::new(&format!(
        "Local\\SerenaDesktop.Codex.{}",
        collision.runtime_instance_id
    )))
    .unwrap();
    let raw = unsafe { CreateJobObjectW(null(), name.as_ptr()) };
    assert!(!raw.is_null());
    let old = unsafe { OwnedHandle::from_raw_handle(raw) };
    assert_eq!(policy(raw), 0);
    for _ in 0..4 {
        let error = launch(&collision).unwrap_err();
        assert_eq!(error.code, "CODEX_JOB_NAME_COLLISION");
        assert!(error.created.is_none());
        assert_eq!(policy(raw), 0);
    }
    drop(old);
    assert_eq!(handle_count(), baseline);

    // Collision with a different kernel-object type is a real CreateJob failure.
    let other = request(&executable);
    let name = wide(OsStr::new(&format!(
        "Local\\SerenaDesktop.Codex.{}",
        other.runtime_instance_id
    )))
    .unwrap();
    let raw = unsafe { CreateEventW(null(), 1, 0, name.as_ptr()) };
    assert!(!raw.is_null());
    let old = unsafe { OwnedHandle::from_raw_handle(raw) };
    assert_eq!(launch(&other).unwrap_err().code, "CODEX_JOB_CREATE_FAILED");
    drop(old);
    for _ in 0..8 {
        let mut invalid = request(&executable);
        invalid.executable = executable.with_file_name("missing.exe");
        let error = launch(&invalid).unwrap_err();
        assert_eq!(error.code, "CODEX_PROCESS_CREATE_FAILED");
        assert!(error.created.is_none());
        assert_eq!(handle_count(), baseline);
    }
    // Inject an error after each acquired resource/attribute and prove RAII cleanup.
    for point in [
        Checkpoint::JobCreated,
        Checkpoint::JobConfigured,
        Checkpoint::StdinCreated,
        Checkpoint::StdoutCreated,
        Checkpoint::StderrCreated,
        Checkpoint::AttributesInitialized,
        Checkpoint::JobAttributeSet,
        Checkpoint::HandleAttributeSet,
    ] {
        for _ in 0..4 {
            let error = launch_inner(&request(&executable), Some(point)).unwrap_err();
            assert_eq!(error.code, "TEST_INJECTED_API_FAILURE");
            assert!(error.created.is_none());
            assert_eq!(handle_count(), baseline, "{point:?}");
        }
    }
    let error = launch_inner(&request(&executable), Some(Checkpoint::ProcessCreated)).unwrap_err();
    assert_eq!(error.code, "CODEX_PROCESS_IDENTITY_FAILED");
    let child = *error.created.unwrap();
    assert_ne!(child.pid, 0);
    assert_eq!(handle_count(), baseline + 5);
    finish(child);
    assert_eq!(handle_count(), baseline);
    for _ in 0..8 {
        finish(launch(&request(&executable)).unwrap().child);
        assert_eq!(handle_count(), baseline);
    }
    // Closing the sole non-inherited Job handle kills the waiting test Child.
    let CreatedChild {
        stdin,
        stdout,
        stderr,
        process,
        job,
        ..
    } = launch(&request(&executable)).unwrap().child;
    drop(job);
    assert_eq!(
        unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) },
        WAIT_OBJECT_0
    );
    drop((stdin, stdout, stderr, process));
    assert_eq!(handle_count(), baseline);
    let mut relative = request(&executable);
    relative.executable = PathBuf::from("child.exe");
    assert_eq!(
        launch(&relative).unwrap_err().code,
        "CODEX_LAUNCH_INPUT_INVALID"
    );
}
