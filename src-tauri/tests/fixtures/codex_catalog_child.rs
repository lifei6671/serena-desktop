//! Provider configuration_catalog 测试专用 fake Codex CLI/app-server。
use std::{
    fs,
    io::{self, BufRead, Write},
    path::Path,
    time::{Duration, Instant},
};

/// 从 JSON-RPC 请求中提取测试使用的数字 id。
fn request_id(line: &str) -> &str {
    line.split("\"id\":")
        .nth(1)
        .unwrap()
        .split([',', '}'])
        .next()
        .unwrap()
}

/// 输出一行 JSON-RPC response 并立即 flush。
fn reply(id: &str, body: &str, error: bool) {
    let key = if error { "error" } else { "result" };
    println!("{{\"id\":{id},\"{key}\":{body}}}");
    io::stdout().flush().unwrap();
}

/// fake 同时实现 compatibility CLI 与 stdio app-server，所有文件都限制在临时目录。
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["--version"] {
        println!("codex-cli 0.153.4");
        return;
    }
    if args.starts_with(&[
        "app-server".into(),
        "generate-json-schema".into(),
        "--experimental".into(),
        "--out".into(),
    ]) {
        let out = Path::new(&args[4]);
        fs::create_dir_all(out).unwrap();
        fs::copy(
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .join("schema.json"),
            out.join("codex_app_server_protocol.schemas.json"),
        )
        .unwrap();
        return;
    }
    assert_eq!(args, ["app-server", "--listen", "stdio://"]);
    fs::write("peer-pid.txt", std::process::id().to_string()).unwrap();
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        if line.contains("\"method\":\"initialize\"") {
            reply(
                request_id(&line),
                r#"{"userAgent":"fake","codexHome":"isolated","platformFamily":"windows","platformOs":"windows"}"#,
                false,
            );
        } else if line.contains("\"method\":\"initialized\"") {
            continue;
        } else if line.contains("\"method\":\"model/list\"") {
            let deadline = Instant::now() + Duration::from_secs(15);
            while !Path::new("release-model-list").exists() {
                assert!(Instant::now() < deadline, "model/list gate timed out");
                std::thread::sleep(Duration::from_millis(5));
            }
            if Path::new("catalog-error").exists() {
                reply(
                    request_id(&line),
                    r#"{"code":-32603,"message":"fake catalog error"}"#,
                    true,
                );
            } else {
                reply(
                    request_id(&line),
                    r#"{"data":[{"id":"preset-visible","model":"visible-wire","displayName":"Visible","description":"","isDefault":true,"hidden":false,"defaultReasoningEffort":"high","supportedReasoningEfforts":[{"reasoningEffort":"high","description":""}]}],"nextCursor":null}"#,
                    false,
                );
            }
        } else {
            panic!("unexpected request: {line}");
        }
    }
}
