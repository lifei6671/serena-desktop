//! CB7-003 native fake peer；仅测试文件驱动，不访问真实 CodeBuddy。
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, Write},
    path::Path,
    time::{Duration, Instant},
};

/// 回复使用物理收到的 SDK id；不存在自造产品请求路由。
fn reply(id: &str, body: &str, error: bool) {
    let key = if error { "error" } else { "result" };
    println!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"{key}\":{body}}}");
    io::stdout().flush().unwrap();
}

/// gate 保证测试可以检查 pending 状态，失败时有界退出。
fn gate(name: &str) {
    let end = Instant::now() + Duration::from_secs(15);
    while !Path::new(name).exists() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 逐行输出可产生超过单次 queue TTL 的长请求，验证实时消费。
fn updates(name: &str, delay: u64) {
    if let Ok(rows) = fs::read_to_string(name) {
        for row in rows.lines() {
            println!("{row}");
            io::stdout().flush().unwrap();
            std::thread::sleep(Duration::from_millis(delay));
        }
    }
}

/// 所有 wire 日志仅写隔离测试 cwd，peer 不解析或推导 Prompt identity。
fn main() {
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open("wire.jsonl")
        .unwrap();
    let mut lines = io::stdin().lock().lines();
    while let Some(line) = lines.next() {
        let line = line.unwrap();
        writeln!(log, "{line}").unwrap();
        log.flush().unwrap();
        let id = line
            .split("\"id\":")
            .nth(1)
            .unwrap()
            .split([',', '}'])
            .next()
            .unwrap();
        if line.contains("\"method\":\"initialize\"") {
            reply(id, r#"{"protocolVersion":1}"#, false);
        } else if line.contains("\"method\":\"session/new\"") {
            reply(id, &fs::read_to_string("new.json").unwrap(), false);
        } else if line.contains("\"method\":\"session/prompt\"") {
            // acceptance 文件由同步 sink 写出，物理请求不允许先于 acceptance。
            assert!(Path::new("accepted").exists());
            let behavior = fs::read_to_string("behavior").unwrap_or_default();
            if Path::new("permission.jsonl").exists() {
                updates("permission.jsonl", 0);
                let response = lines.next().unwrap().unwrap();
                writeln!(log, "{response}").unwrap();
                log.flush().unwrap();
            }
            if behavior == "gate" {
                gate("release-prompt");
            }
            if behavior == "eof" {
                return;
            }
            if behavior == "timeout" {
                std::thread::sleep(Duration::from_secs(30));
                return;
            }
            if behavior == "error" {
                reply(
                    id,
                    r#"{"code":-32603,"message":"provider raw secret"}"#,
                    true,
                );
                continue;
            }
            updates("updates.jsonl", if behavior == "stream" { 20 } else { 0 });
            let body = fs::read_to_string("response.json").unwrap();
            reply(id, &body, false);
            if behavior == "duplicate" {
                reply(id, &body, false);
            }
            if Path::new("late.jsonl").exists() {
                gate("release-late");
                updates("late.jsonl", 0);
                fs::write("late-sent", "").unwrap();
            }
        } else {
            panic!("unexpected method");
        }
    }
}
