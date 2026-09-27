//! 外部进程集成测试：SDK 与真实匿名管道，不使用内存 transport 代替。
use serde_json::Value;
use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
/// 每个 fixture 均在独立 harness 中执行，外层测试也设定有界超时。
fn run(case: &str) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("result.json");
    let peer_pid_file = dir.path().join("peer.pid");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cb5-002-initialize-probe"))
        .args(["fixture", output.to_str().unwrap(), case])
        .env("CB5_TEST_PEER_PID_FILE", &peer_pid_file)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if start.elapsed() > Duration::from_secs(12) {
            let errors = terminate_owned_tree(&mut child, &peer_pid_file);
            panic!("fixture exceeded bounded wait; cleanup: {errors:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let value: Value = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!(value["cleanup"]["waited"], true);
    assert_eq!(value["cleanup"]["directChildReaped"], true);
    value
}
/// 对端在请求前关闭也必须稳定结束。
#[test]
fn eof_before() {
    let r = run("eof-before");
    assert_eq!(r["initializeSucceeded"], false);
    assert!(
        r["error"]
            .as_str()
            .unwrap()
            .contains("incoming_transport_closed")
    );
}
/// 请求送达后 EOF，不能挂住或通过 gate。
#[test]
fn eof_during() {
    let r = run("eof-during");
    assert_eq!(r["protocolCompatible"], false);
    assert!(
        r["error"]
            .as_str()
            .unwrap()
            .contains("incoming_transport_closed")
    );
}
/// 数字999可以被官方 SDK deserialize，但被 Serena v1 gate 拒绝。
#[test]
fn mismatch() {
    let r = run("mismatch");
    assert_eq!(r["initializeSucceeded"], true);
    assert_eq!(r["response"]["protocolVersion"], 999);
    assert_eq!(r["protocolCompatible"], false);
}
/// 缺能力不降低协议兼容，正常关流后子进程退出。
#[test]
fn missing_capability_and_clean_shutdown() {
    let r = run("missing-capability");
    assert_eq!(r["protocolCompatible"], true);
    assert_eq!(r["response"]["agentCapabilities"]["loadSession"], false);
    assert_eq!(r["cleanup"]["terminated"], false);
    assert_eq!(r["cleanup"]["exitCode"], 0);
}
/// 不响应且不退出的 peer 必须 timeout、terminate 并 wait。
#[test]
fn timeout_cleanup() {
    let r = run("hang");
    assert_eq!(r["error"], "INITIALIZE_TIMEOUT");
    assert_eq!(r["cleanup"]["terminated"], true);
    assert!(r["totalElapsedMs"].as_u64().unwrap() < 9000);
}

// Windows 测试专用进程句柄 API；不实现生产 Job 或未知进程枚举。
#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, ms: u32) -> u32;
    fn TerminateProcess(handle: *mut std::ffi::c_void, code: u32) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}
/// 同步测试 watchdog 使用有限轮询，不把 child.wait 无限挂起。
fn wait_bounded(child: &mut std::process::Child, limit: Duration) -> std::io::Result<bool> {
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(true);
        }
        if start.elapsed() >= limit {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(10));
    }
}
/// 仅清理当前测试持有的 harness 树和已知 fake peer；错误不跳过后续 wait。
fn terminate_owned_tree(
    child: &mut std::process::Child,
    peer_file: &std::path::Path,
) -> Vec<String> {
    let mut errors = Vec::new();
    let peer_handle = std::fs::read_to_string(peer_file)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(|pid| unsafe { OpenProcess(0x00100001, 0, pid) })
        .unwrap_or(std::ptr::null_mut());
    if peer_handle.is_null() {
        errors.push(format!(
            "peer handle unavailable: {}",
            std::io::Error::last_os_error()
        ));
    }
    let taskkill = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/taskkill.exe");
    match Command::new(&taskkill)
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut killer) => {
            if !matches!(wait_bounded(&mut killer, Duration::from_secs(3)), Ok(true)) {
                errors.push("tree taskkill timed out or wait failed".into());
                let _ = killer.kill();
                let _ = wait_bounded(&mut killer, Duration::from_secs(2));
            }
        }
        Err(error) => errors.push(format!("tree taskkill spawn: {error}")),
    }
    // 已知 peer handle 在杀树前取得；树操作不生效时直接终止这个已验证的 fixture。
    if !peer_handle.is_null() {
        if unsafe { WaitForSingleObject(peer_handle, 0) } != 0 {
            if unsafe { TerminateProcess(peer_handle, 1) } == 0 {
                errors.push(format!(
                    "peer terminate: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
        if unsafe { WaitForSingleObject(peer_handle, 2000) } != 0 {
            errors.push("peer exit not observed".into());
        }
        unsafe { CloseHandle(peer_handle) };
    }
    if !matches!(wait_bounded(child, Duration::from_secs(2)), Ok(true)) {
        if let Err(error) = child.kill() {
            errors.push(format!("harness kill: {error}"));
        }
        if !matches!(wait_bounded(child, Duration::from_secs(2)), Ok(true)) {
            errors.push("harness not reaped".into());
        }
    }
    errors
}
/// 抢先终止 harness，直接持有 peer Windows handle 验证后代也退出。
#[test]
#[cfg(windows)]
fn watchdog_reaps_owned_peer_tree() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("peer.pid");
    let output = dir.path().join("result.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cb5-002-initialize-probe"))
        .args(["fixture", output.to_str().unwrap(), "hang"])
        .env("CB5_TEST_PEER_PID_FILE", &file)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let peer = loop {
        if let Ok(value) = std::fs::read_to_string(&file) {
            if let Ok(pid) = value.parse::<u32>() {
                break Some(pid);
            }
        }
        if start.elapsed() > Duration::from_secs(2) {
            break None;
        }
        thread::sleep(Duration::from_millis(5));
    };
    let handle = peer
        .map(|pid| unsafe { OpenProcess(0x00100000, 0, pid) })
        .unwrap_or(std::ptr::null_mut());
    let errors = terminate_owned_tree(&mut child, &file);
    let exited = if handle.is_null() {
        false
    } else {
        let result = unsafe { WaitForSingleObject(handle, 2000) };
        unsafe { CloseHandle(handle) };
        result == 0
    };
    assert!(errors.is_empty(), "{errors:?}");
    assert!(
        exited,
        "owned peer process did not signal exit: peer={peer:?}, handle={handle:?}"
    );
}
