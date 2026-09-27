//! 仅 native Fresh Session 测试的 fake ACP peer；绝不处理 session/prompt。
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, Write},
    time::{Duration, Instant},
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
    fn IsProcessInJob(
        process: *mut std::ffi::c_void,
        job: *mut std::ffi::c_void,
        member: *mut i32,
    ) -> i32;
}

/// 回复只回显官方 SDK 生成的 id，不承担产品 request 分发 authority。
fn reply(id: &str, result: &str, error: bool) {
    let key = if error { "error" } else { "result" };
    println!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"{key}\":{result}}}");
    io::stdout().flush().unwrap();
}

/// 测试编排通过本次隔离 cwd 的文件控制应答；所有等待都有上限。
fn main() {
    // 仅 fixture 的固定断言错误留在临时目录，方便区分 peer 失败与产品 transport 失败。
    std::panic::set_hook(Box::new(|info| {
        let _ = fs::write("peer-panic.txt", info.to_string());
    }));
    let mut member = 0;
    // SAFETY: 当前 process 及输出指针在调用期间有效。
    assert_ne!(
        unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut member) },
        0
    );
    assert_eq!(member, 1);
    let executable = std::env::current_exe().unwrap();
    let mode = executable
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix("cb7-fresh-")
        .unwrap();
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open("wire.jsonl")
        .unwrap();
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        writeln!(log, "{line}").unwrap();
        log.flush().unwrap();
        let method = if line.contains("\"method\":\"initialize\"") {
            "initialize"
        } else if line.contains("\"method\":\"session/new\"") {
            "new"
        } else if line.contains("\"method\":\"session/set_mode\"") {
            "mode"
        } else if line.contains("\"method\":\"session/set_config_option\"") {
            "config"
        } else {
            panic!("unexpected method: prompt is forbidden")
        };
        let id = line
            .split("\"id\":")
            .nth(1)
            .unwrap()
            .split([',', '}'])
            .next()
            .unwrap();
        if mode == "gated" {
            let deadline = Instant::now() + Duration::from_secs(15);
            while !std::path::Path::new(&format!("release-{method}")).exists() {
                assert!(Instant::now() < deadline, "test did not release gate");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        if mode == format!("{method}-timeout") {
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        if mode == format!("{method}-eof") {
            return;
        }
        if mode == format!("{method}-error") {
            reply(id, r#"{"code":-32603,"message":"fake failure"}"#, true);
            continue;
        }
        match method {
            "initialize" => reply(
                id,
                if mode == "mismatch" {
                    r#"{"protocolVersion":2}"#
                } else {
                    r#"{"protocolVersion":1}"#
                },
                false,
            ),
            "new" => {
                if let Ok(early) = fs::read_to_string("early.jsonl") {
                    print!("{early}");
                }
                reply(id, &fs::read_to_string("new.json").unwrap(), false);
            }
            "mode" => reply(id, "{}", false),
            "config" => reply(id, &fs::read_to_string("config.json").unwrap(), false),
            _ => unreachable!(),
        }
    }
}
