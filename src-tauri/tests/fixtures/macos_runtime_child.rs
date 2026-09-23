use std::{
    env, fs,
    io::{BufRead, Read, Write},
    process::{exit, Command},
    thread,
    time::Duration,
};

unsafe extern "C" {
    fn getpid() -> i32;
    fn getpgrp() -> i32;
    fn getsid(pid: i32) -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
    fn signal(signal: i32, handler: usize) -> usize;
}

const SIGTERM: i32 = 15;
const SIGSTOP: i32 = 17;
const SIG_IGN: usize = 1;

/// 仅让 macOS managed compatibility 测试的短命 CLI 在输出/退出前停住，
/// 直到父测试完成 Runtime 身份采集后发送 SIGCONT。
fn pause_owned_probe_fixture() {
    let is_owned_probe = env::current_exe()
        .ok()
        .and_then(|path| path.file_name().map(|name| name.to_owned()))
        .is_some_and(|name| name.to_string_lossy().starts_with("probe-ownership-child"));
    if !is_owned_probe {
        return;
    }

    // SAFETY: 测试 fixture 只向自身发送不可捕获的 SIGSTOP；父测试持有 PID 并负责 SIGCONT/收口。
    let result = unsafe { kill(getpid(), SIGSTOP) };
    assert_eq!(result, 0, "pause probe fixture");
}

/// 让当前测试进程忽略 SIGTERM，供强制收口场景使用。
fn ignore_term() {
    // SAFETY: fixture 只将当前进程的 SIGTERM disposition 设置为系统定义的 SIG_IGN。
    unsafe {
        signal(SIGTERM, SIG_IGN);
    }
}

/// 以低频休眠保持 fixture 存活，直到测试发送信号。
fn wait_forever() -> ! {
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

/// 生成 leaf，并通过 marker 文件把其 PID 交给父测试。
fn spawn_leaf(marker: &str, leaf_mode: &str) {
    let leaf = Command::new(env::current_exe().expect("fixture executable"))
        .args(["leaf", leaf_mode, marker])
        .spawn()
        .expect("spawn fixture leaf");
    fs::write(marker, leaf.id().to_string()).expect("write leaf marker");
}

/// 固定进程 fixture 入口，仅使用 argv 分派模式，不经过 shell。
fn main() {
    let mut args = env::args().skip(1);
    let mode = args.next().unwrap_or_default();
    match mode.as_str() {
        "--version" => {
            pause_owned_probe_fixture();
            // compatibility probe 的版本输出有效；专用文件名模拟有输出但退出码非零。
            println!("codex-cli fixture 1.0.0");
            if env::current_exe()
                .expect("fixture executable")
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with("version-nonzero"))
            {
                exit(7);
            }
        }
        "report" => {
            pause_owned_probe_fixture();
            let argument = args.next().unwrap_or_default();
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .expect("read fixture stdin");
            // SAFETY: 无参数的身份查询只读取当前 fixture 的内核进程状态。
            let (pid, pgid, sid) = unsafe { (getpid(), getpgrp(), getsid(0)) };
            println!(
                "pid={pid};pgid={pgid};sid={sid};cwd={};arg={argument};stdin={input}",
                env::current_dir().expect("fixture cwd").display()
            );
            eprintln!("stderr-ready");
        }
        "probe-output" => {
            pause_owned_probe_fixture();
            // Compatibility CLI fixture 不读 stdin，模拟 version/schema 命令正常退出。
            println!("{}", args.next().unwrap_or_else(|| "probe-ok".into()));
        }
        "app-server" => {
            if args.next().as_deref() == Some("generate-json-schema") {
                pause_owned_probe_fixture();
                // 写出确定性不兼容的 schema，让测试验证共享契约拒绝而非 unknown mode。
                assert_eq!(args.next().as_deref(), Some("--experimental"));
                assert_eq!(args.next().as_deref(), Some("--out"));
                let directory = args.next().expect("schema output directory");
                fs::create_dir_all(&directory).expect("create schema directory");
                fs::write(
                    std::path::Path::new(&directory)
                        .join("codex_app_server_protocol.schemas.json"),
                    b"{\"definitions\":{}}",
                )
                .expect("write incompatible schema");
                if env::current_exe()
                    .expect("fixture executable")
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with("schema-nonzero"))
                {
                    exit(9);
                }
                return;
            }
            // 只实现 initialize/initialized smoke 所需的最小 JSONL Contract，不启动 Turn。
            let stdin = std::io::stdin();
            let mut lines = stdin.lock().lines();
            let request = lines.next().expect("initialize request").expect("read initialize");
            assert!(request.contains("\"method\":\"initialize\""));
            println!(
                "{{\"id\":1,\"result\":{{\"userAgent\":\"fixture\",\"codexHome\":\"fixture\",\"platformFamily\":\"macos\",\"platformOs\":\"macos\"}}}}"
            );
            std::io::stdout().flush().expect("flush initialize response");
            let initialized = lines.next().expect("initialized notification").expect("read initialized");
            assert!(initialized.contains("\"method\":\"initialized\""));
            wait_forever();
        }
        "tree" | "ignore-tree" | "leader-term-exit" => {
            // ignore-tree 的 leader 与 leaf 都忽略 SIGTERM；leader-term-exit 仅让 leaf 忽略。
            if mode == "ignore-tree" {
                ignore_term();
            }
            let marker = args.next().expect("marker path");
            let leaf_mode = if mode == "tree" { "default" } else { "ignore" };
            spawn_leaf(&marker, leaf_mode);
            wait_forever();
        }
        "leader-exit" => {
            let marker = args.next().expect("marker path");
            spawn_leaf(&marker, "ignore");
            let mut release = [0_u8; 1];
            std::io::stdin()
                .read_exact(&mut release)
                .expect("read leader release byte");
            exit(0);
        }
        "leader-only-exit" => {
            // 无后代 leader 由测试释放后正常退出，用于验证 live group-empty 完成路径。
            let marker = args.next().expect("marker path");
            fs::write(format!("{marker}.ready"), b"ready").expect("write leader ready marker");
            let mut release = [0_u8; 1];
            std::io::stdin()
                .read_exact(&mut release)
                .expect("read leader release byte");
            exit(0);
        }
        "leaf" => {
            if args.next().as_deref() == Some("ignore") {
                ignore_term();
            }
            let marker = args.next().expect("marker path");
            fs::write(format!("{marker}.ready"), b"ready").expect("write leaf ready marker");
            wait_forever();
        }
        _ => exit(2),
    }
}
