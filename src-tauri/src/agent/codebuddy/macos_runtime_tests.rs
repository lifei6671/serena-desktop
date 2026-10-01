//! 默认使用本机编译的原生 fake ACP，不依赖 CodeBuddy 安装或账号。
use super::*;
use crate::agent::{
    codebuddy::{discovery::ResolvedLaunchSpec, macos_launcher::UncCurrentDirectoryPolicy},
    codex::macos_launcher::{MacosProcessIdentityAdapter, process_group_members},
};
use std::{path::Path, process::Command, time::Instant};

/// 编译最小 ndJSON initialize peer；只验证 transport、超时和 containment。
fn fixture(directory: &Path) -> std::path::PathBuf {
    let source = directory.join("fixture.rs");
    std::fs::write(&source, r#"
use std::{io::{self, BufRead, Write}, sync::atomic::{AtomicBool, Ordering}};
static STOPPED: AtomicBool = AtomicBool::new(false);
unsafe extern "C" { fn signal(number: i32, handler: usize) -> usize; }
/// leader 保持存活直到直接后代被回收，便于观测真实 group-empty。
extern "C" fn stop(_: i32) { STOPPED.store(true, Ordering::SeqCst); }
/// fake peer 写下真实 PID 后按测试模式响应 initialize。
fn main() {
    if std::env::args().any(|arg| arg == "--leaf") {
        loop { std::thread::sleep(std::time::Duration::from_millis(20)); }
    }
    std::fs::write("leader.pid", std::process::id().to_string()).unwrap();
    let executable = std::env::current_exe().unwrap();
    let mode = executable.file_stem().unwrap().to_str().unwrap();
    let mut leaf = std::process::Command::new(&executable).arg("--leaf").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    std::fs::write("leaf.pid", leaf.id().to_string()).unwrap();
    unsafe { signal(15, stop as *const () as usize); }
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).unwrap();
    if mode == "timeout" {
        while !STOPPED.load(Ordering::SeqCst) { std::thread::sleep(std::time::Duration::from_millis(20)); }
        let _ = leaf.wait();
        return;
    }
    let id = line.split("\"id\":").nth(1).unwrap().split([',','}']).next().unwrap();
    let version = if mode == "mismatch" { 2 } else { 1 };
    println!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"protocolVersion\":{version},\"agentInfo\":{{\"name\":\"native-fixture\",\"version\":\"1\"}}}}}}");
    io::stdout().flush().unwrap();
    while !STOPPED.load(Ordering::SeqCst) { std::thread::sleep(std::time::Duration::from_millis(20)); }
    let _ = leaf.wait();
}
"#).unwrap();
    let executable = directory.join("base");
    let result = Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "codebuddy_macos_fixture"])
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    executable
}

/// 等待真实内核 group-empty，不能把请求终止当成 PASS。
async fn assert_empty(pgid: i32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if process_group_members(pgid).unwrap().is_empty() {
            return;
        }
        assert!(Instant::now() < deadline, "fixture group {pgid} survived");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
/// 成功、Drop、握手不兼容与超时均须收口同一个原生进程组。
async fn codebuddy_macos_managed_handshake_and_cleanup() {
    let directory = tempfile::tempdir().unwrap();
    let base = fixture(directory.path());
    for mode in ["success", "drop", "mismatch", "timeout"] {
        // 每轮只接受当前子进程写入的标记，禁止沿用前一轮 PID。
        for marker in ["leader.pid", "leaf.pid"] {
            match std::fs::remove_file(directory.path().join(marker)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!("remove fixture marker: {error}"),
            }
        }
        let executable = directory.path().join(mode);
        std::fs::copy(&base, &executable).unwrap();
        let cwd = crate::config::canonicalize_workspace_root(directory.path()).unwrap();
        let request = LaunchRequest::from_resolved(
            &ResolvedLaunchSpec {
                executable,
                args: vec!["--acp".into()],
                path_projection: vec![directory.path().into()],
            },
            &cwd,
            UncCurrentDirectoryPolicy::Unsupported,
            format!("cb-macos-fixture-{mode}"),
        )
        .unwrap();
        // 编译后首次原生执行在整套测试负载下可能超过 250ms；超时场景仍明确等待有界 2s。
        let limits = Limits {
            request_timeout: Duration::from_secs(2),
            ..Limits::default()
        };
        let result = Runtime::start(request, limits).await;
        let failure_code = result.as_ref().err().map(|failure| failure.code());
        match mode {
            "success" | "drop" => {
                assert!(result.is_ok(), "mode={mode}, initialize={failure_code:?}")
            }
            "mismatch" => assert_eq!(
                failure_code,
                Some(Failure::Incompatible.code()),
                "mode={mode}"
            ),
            "timeout" => assert_eq!(failure_code, Some(Failure::Timeout.code()), "mode={mode}"),
            _ => unreachable!(),
        }
        let pid: i32 = std::fs::read_to_string(directory.path().join("leader.pid"))
            .unwrap_or_else(|error| {
                panic!("mode={mode}, initialize={failure_code:?}, missing leader marker: {error}")
            })
            .parse()
            .unwrap();
        match mode {
            "success" | "drop" => {
                let (runtime, _) = result.unwrap_or_else(|failure| panic!("{}", failure.code()));
                let leaf_pid: i32 = std::fs::read_to_string(directory.path().join("leaf.pid"))
                    .unwrap()
                    .parse()
                    .unwrap();
                let leaf = MacosProcessIdentityAdapter::observe(leaf_pid).unwrap();
                assert_eq!((leaf.pgid, leaf.sid), (pid, pid));
                assert!(process_group_members(pid).unwrap().contains(&leaf_pid));
                let identity = MacosProcessIdentityAdapter::observe(pid).unwrap();
                assert_eq!((identity.pid, identity.pgid, identity.sid), (pid, pid, pid));
                assert!(
                    identity
                        .start_token
                        .encode()
                        .starts_with("darwin_proc_bsd_start_v1:")
                );
                if mode == "drop" {
                    drop(runtime);
                } else {
                    runtime.shutdown().await.unwrap();
                }
            }
            "mismatch" => assert!(matches!(result, Err(Failure::Incompatible))),
            "timeout" => assert!(matches!(result, Err(Failure::Timeout))),
            _ => unreachable!(),
        }
        assert_empty(pid).await;
    }
}

#[tokio::test]
/// 身份不匹配时禁止信号，并跨最后一次 Drop 保留真实 owner、阻止同 Workspace 重启。
async fn codebuddy_macos_failed_cleanup_retains_owner() {
    use crate::agent::codex::macos_launcher::MacosLaunchRequest;
    let directory = tempfile::tempdir().unwrap();
    let workspace = crate::config::canonicalize_workspace_root(directory.path()).unwrap();
    let mut core = MacosRuntime::create_external(
        MacosLaunchRequest {
            executable: "/bin/sleep".into(),
            args: vec!["60".into()],
            current_dir: workspace.clone(),
            runtime_instance_id: "quarantine-fixture".into(),
        },
        std::ffi::OsStr::new("/usr/bin:/bin"),
    )
    .unwrap();
    let identity = core.process_identity().clone();
    let mut mismatch = identity.clone();
    mismatch.start_token.seconds += 1;
    core.replace_identity_for_test(mismatch);
    let owner = Owner {
        core: Mutex::new(Some(core)),
        workspace: workspace.clone(),
        durable: None,
        outcome: Mutex::new(None),
        cleanup_on_drop: true,
    };
    drop(owner);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !QUARANTINE.lock().unwrap().contains_key(&workspace) {
        assert!(Instant::now() < deadline, "failed owner was not retained");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(MacosProcessIdentityAdapter::observe(identity.pid).is_ok());
    let request = LaunchRequest::from_resolved(
        &ResolvedLaunchSpec {
            executable: "/bin/sleep".into(),
            args: vec!["--acp".into()],
            path_projection: vec!["/bin".into()],
        },
        &workspace,
        UncCurrentDirectoryPolicy::Unsupported,
        "blocked-fixture".into(),
    )
    .unwrap();
    assert!(matches!(
        Runtime::start(request, Limits::default()).await,
        Err(Failure::Cleanup)
    ));
    // 仅 fixture 恢复此前保存的真实身份并显式收口，不为产品 Runtime 补造证据。
    let retained = QUARANTINE.lock().unwrap().remove(&workspace).unwrap();
    for ownership in retained {
        let RetainedOwnership::Managed(mut core) = ownership else {
            panic!("expected managed owner");
        };
        core.replace_identity_for_test(identity.clone());
        tokio::task::spawn_blocking(move || {
            core.shutdown(Duration::from_millis(500), Duration::from_secs(2))
        })
        .await
        .unwrap()
        .unwrap();
    }
    assert_empty(identity.pgid).await;
}

#[test]
/// post-create 身份 acquisition 失败须保留 child，不把直接句柄 Drop 当作整组收口。
fn codebuddy_macos_unverified_created_child_is_retained() {
    use crate::agent::codex::macos_launcher::{MacosLaunchRequest, launch};
    let directory = tempfile::tempdir().unwrap();
    let workspace = crate::config::canonicalize_workspace_root(directory.path()).unwrap();
    let launched = launch(&MacosLaunchRequest {
        executable: "/bin/sleep".into(),
        args: vec!["60".into()],
        current_dir: workspace.clone(),
        runtime_instance_id: "created-quarantine-fixture".into(),
    })
    .unwrap();
    let identity = launched.identity;
    retain_created(workspace.clone(), launched.child);
    assert!(QUARANTINE.lock().unwrap().contains_key(&workspace));
    assert!(identity.matches(&MacosProcessIdentityAdapter::observe(identity.pid).unwrap()));
    let retained = QUARANTINE.lock().unwrap().remove(&workspace).unwrap();
    for ownership in retained {
        let RetainedOwnership::Created(mut child) = ownership else {
            panic!("expected created owner");
        };
        // SAFETY: 仅测试 teardown 使用刚重新验证的同一 fixture Session，绝不生成产品 evidence。
        assert_eq!(unsafe { libc::killpg(identity.pgid, libc::SIGKILL) }, 0);
        child.process.wait().unwrap();
        assert!(process_group_members(identity.pgid).unwrap().is_empty());
    }
}
