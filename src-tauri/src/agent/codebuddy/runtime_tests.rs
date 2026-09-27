//! 仅 Windows fake fixture 验证 managed Job + SDK 接线，无真实 CodeBuddy。
use super::super::{discovery::ResolvedLaunchSpec, windows_launcher::UncCurrentDirectoryPolicy};
use super::*;
use std::{
    io::Read,
    mem::{size_of, zeroed},
    os::windows::process::CommandExt,
    path::Path,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{JobObjects::*, Threading::*},
};

/// 真实查询 Job，而不是把主 PID 已退出视为整树结束。
fn active(job: &OwnedHandle) -> u32 {
    let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
    // SAFETY: 测试独占有效 Job handle，buffer 长度与 ABI 一致。
    assert_ne!(
        unsafe {
            QueryInformationJobObject(
                job.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        },
        0
    );
    info.ActiveProcesses
}

/// 终止要求精确归零；启动只要求最低成员数，系统辅助进程不破坏 containment 契约。
async fn wait_count(job: &OwnedHandle, expected: u32) -> Result<(), u32> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let count = active(job);
        if count == expected || (expected > 0 && count > expected) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(active(job));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// 从公开冻结 resolver 形状构造请求，不绕过 launcher policy。
fn request(executable: &Path, sequence: usize) -> LaunchRequest {
    let resolved = ResolvedLaunchSpec {
        executable: executable.into(),
        args: vec!["--acp".into()],
        path_projection: vec![executable.parent().unwrap().into()],
    };
    let root = crate::config::canonicalize_workspace_root(executable.parent().unwrap()).unwrap();
    LaunchRequest::from_resolved(
        &resolved,
        &root,
        UncCurrentDirectoryPolicy::Unsupported,
        format!("cb6-003-fixture-{}-{sequence}", std::process::id()),
    )
    .unwrap()
}

#[tokio::test]
/// 官方 SDK 消费真实 managed Win32 pipes；失败、shutdown、drop、post-create error 均整树结束。
async fn managed_job_handshake_and_failure_cleanup() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().join("base.exe");
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codebuddy_acp_child.rs");
    let build = std::process::Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "codebuddy_acp_child"])
        .arg(source)
        .arg("-o")
        .arg(&base)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    for (sequence, mode) in [
        "success",
        "mismatch",
        "timeout",
        "eof",
        "drop",
        "post-create",
        "transport",
    ]
    .into_iter()
    .enumerate()
    {
        let executable = directory.path().join(format!("{mode}.exe"));
        std::fs::copy(&base, &executable).unwrap();
        let child = windows_launcher::launch(&request(&executable, sequence))
            .unwrap()
            .child;
        let job = child.job.try_clone().unwrap();
        let process = child.process.try_clone().unwrap();
        if mode == "eof" {
            // SAFETY: 明确建立 initialize 前 peer 已退出，后代仍由同一 Job 持有。
            assert_eq!(
                unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) },
                WAIT_OBJECT_0
            );
            use std::io::Write;
            let error = (&child.stdin)
                .write_all(b"fixture-closed-pipe")
                .unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
        if let Err(count) = wait_count(&job, if mode == "eof" { 1 } else { 2 }).await {
            // SAFETY: 仅终止本次 fixture 的 owned Job，随后读取有界 fake stderr 辅助定位。
            unsafe {
                TerminateJobObject(job.as_raw_handle(), 1);
            }
            wait_count(&job, 0).await.unwrap();
            let mut error = String::new();
            (&child.stderr)
                .take(4096)
                .read_to_string(&mut error)
                .unwrap();
            panic!("fixture {mode}: Job active={count}, fake stderr={error}");
        }
        if mode == "post-create" {
            cleanup_launch_error(LaunchError {
                code: "FIXTURE_POST_CREATE",
                win32_error: 1,
                created: Some(Box::new(child)),
            });
        } else {
            let result = Runtime::from_child(
                child,
                Limits {
                    request_timeout: Duration::from_millis(500),
                    stderr_bytes: 64,
                    ..Limits::default()
                },
            )
            .await;
            match mode {
                "success" | "drop" | "transport" => {
                    let (runtime, handshake) = result.unwrap();
                    assert_eq!(
                        handshake.response.agent_info.unwrap().name,
                        "fixture-in-job"
                    );
                    assert_eq!(runtime._tail.lock().unwrap().bytes().len(), 64);
                    if mode == "transport" {
                        let failure = runtime
                            .client
                            .as_ref()
                            .unwrap()
                            .requests
                            .request(
                                agent_client_protocol::UntypedMessage::new(
                                    "fixture/fail",
                                    serde_json::json!({}),
                                )
                                .unwrap(),
                            )
                            .await;
                        assert_eq!(failure, Err(Failure::InvalidJson));
                        wait_count(&job, 0).await.unwrap();
                        runtime.shutdown().await.unwrap();
                    } else if mode == "drop" {
                        drop(runtime);
                    } else {
                        runtime.shutdown().await.unwrap();
                    }
                }
                "mismatch" => assert!(matches!(result, Err(Failure::Incompatible))),
                "timeout" => assert!(matches!(result, Err(Failure::Timeout))),
                "eof" => assert_eq!(result.err(), Some(Failure::Eof)),
                _ => unreachable!(),
            }
        }
        wait_count(&job, 0).await.unwrap();
        // SAFETY: duplicate process handle 在整个等待期间有效。
        assert_eq!(
            unsafe { WaitForSingleObject(process.as_raw_handle(), 1000) },
            WAIT_OBJECT_0
        );
    }
}
