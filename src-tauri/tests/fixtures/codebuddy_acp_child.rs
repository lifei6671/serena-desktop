//! 无外部依赖的 Windows ACP fake，只处理 initialize；后代用于验证整 Job 清理。
use std::{
    ffi::c_void,
    io::{self, BufRead, Write},
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    time::Duration,
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn IsProcessInJob(process: *mut c_void, job: *mut c_void, member: *mut i32) -> i32;
    fn GetStdHandle(which: u32) -> *mut c_void;
    fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
}

/// 首个应用操作观察 Job membership；fake main/descendant 都不能在 Job 外运行。
fn main() {
    let mut member = 0;
    assert_ne!(
        unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut member) },
        0
    );
    assert_eq!(member, 1);
    if std::env::args().any(|arg| arg == "--descendant") {
        println!("DESCENDANT_IN_JOB");
        io::stdout().flush().unwrap();
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    let executable = std::env::current_exe().unwrap();
    // ACP 端点只属于 fake peer；禁止它们以额外继承 handle 泄漏给存活后代。
    for which in [-10i32, -11, -12] {
        assert_ne!(
            unsafe { SetHandleInformation(GetStdHandle(which as u32), 1, 0) },
            0
        );
    }
    // 后代使用独立管道，不持有 ACP stdio；单独确认它的 first-operation membership。
    let mut descendant = Command::new(&executable)
        .arg("--descendant")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // 后代也禁止可见 console；Job 可能仍含系统辅助成员，不假设 exact process count。
        .creation_flags(0x0800_0000)
        .spawn()
        .unwrap();
    let mut ready = String::new();
    io::BufReader::new(descendant.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim(), "DESCENDANT_IN_JOB");
    let mode = executable.file_stem().unwrap().to_string_lossy();
    if mode == "eof" {
        return;
    }
    let mut input = io::stdin().lock();
    let mut line = String::new();
    input.read_line(&mut line).unwrap();
    assert!(line.contains("\"method\":\"initialize\""));
    assert!(line.contains("\"protocolVersion\":1"));
    if mode == "timeout" {
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    // SDK UUID 为固定 JSON string；fixture 只提取原文 id，不自行分发产品请求。
    let id = line
        .split("\"id\":")
        .nth(1)
        .unwrap()
        .split([',', '}'])
        .next()
        .unwrap();
    let version = if mode == "mismatch" { 2 } else { 1 };
    eprint!("{}TAIL", "x".repeat(20_000));
    println!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"protocolVersion\":{version},\"agentInfo\":{{\"name\":\"fixture-in-job\",\"version\":\"1\"}}}}}}"
    );
    io::stdout().flush().unwrap();
    // 成功握手后等待 owner 关闭；不接受任何 session/new 或 prompt。
    line.clear();
    let _ = input.read_line(&mut line);
    if mode == "transport" {
        println!("invalid-transport-frame");
        io::stdout().flush().unwrap();
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}
