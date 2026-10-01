use super::*;
use crate::agent::{
    codebuddy::{discovery::DiscoveryResult, provider::register_codebuddy_provider_with_discovery},
    codex::macos_launcher::process_group_members,
    provider::{
        ProviderId,
        registry::{ProviderHealth, ProviderRegistry},
    },
    store::StateStore,
};
use std::process::Command;

/// 原生 fixture 使用真实 --version，超时场景保留子进程并在 SIGTERM 后回收它。
fn fixture(directory: &std::path::Path) -> std::path::PathBuf {
    let source = directory.join("fixture.rs");
    std::fs::write(
        &source,
        r#"
use std::{io::{self, Write}, sync::atomic::{AtomicBool, Ordering}, time::Duration};
static STOP: AtomicBool = AtomicBool::new(false);
unsafe extern "C" { fn signal(sig: i32, handler: usize) -> usize; }
extern "C" fn stop(_: i32) { STOP.store(true, Ordering::SeqCst); }
fn main() {
    let executable = std::env::current_exe().unwrap();
    if std::env::args().any(|arg| arg == "--leaf") {
        unsafe { signal(15, stop as *const () as usize); }
        while !STOP.load(Ordering::SeqCst) { std::thread::sleep(Duration::from_millis(10)); }
        return;
    }
    assert!(std::env::args().any(|arg| arg == "--version"));
    let mode = executable.file_stem().unwrap().to_str().unwrap();
    std::fs::write(executable.with_extension("pid"), std::process::id().to_string()).unwrap();
    match mode {
        "timeout" => {
            unsafe { signal(15, stop as *const () as usize); }
            let mut leaf = Command::new(&executable).arg("--leaf").spawn().unwrap();
            std::fs::write(executable.with_extension("leaf"), leaf.id().to_string()).unwrap();
            while !STOP.load(Ordering::SeqCst) { std::thread::sleep(Duration::from_millis(10)); }
            let _ = leaf.wait();
        },
        "failure" => { println!("2.160.0"); std::process::exit(1); },
        "malformed" => println!("ACP 1 build abc"),
        "overflow" => { print!("{}", "x".repeat(32768)); io::stdout().flush().unwrap(); },
        _ => println!("2.160.0"),
    }
}
use std::process::Command;
"#,
    )
    .unwrap();
    let executable = directory.join("base");
    let output = Command::new("rustc")
        .args([
            "--edition=2024",
            "--crate-name",
            "codebuddy_version_fixture",
        ])
        .arg(&source)
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

/// 成功、失败、超限与超时都必须得到 direct child 回收且原进程组为空的证据。
#[test]
fn version_probe_success_failure_timeout_and_cleanup() {
    let directory = tempfile::tempdir().unwrap();
    let base = fixture(directory.path());
    for mode in ["success", "failure", "malformed", "overflow", "timeout"] {
        let executable = directory.path().join(mode);
        std::fs::copy(&base, &executable).unwrap();
        let resolved = ResolvedLaunchSpec {
            executable: executable.canonicalize().unwrap(),
            args: vec!["--acp".into()],
            path_projection: vec![],
        };
        let started = Instant::now();
        let version = probe_product_version(&resolved, Duration::from_secs(1));
        assert_eq!(
            version.as_deref(),
            (mode == "success").then_some("2.160.0"),
            "{mode}"
        );
        assert!(started.elapsed() < Duration::from_secs(4), "{mode} blocked");
        let pid: i32 = std::fs::read_to_string(executable.with_extension("pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            process_group_members(pid).unwrap().is_empty(),
            "{mode}: group {pid} survived"
        );
        if mode == "timeout" {
            let leaf: i32 = std::fs::read_to_string(executable.with_extension("leaf"))
                .unwrap()
                .parse()
                .unwrap();
            assert!(
                crate::agent::codex::macos_launcher::MacosProcessIdentityAdapter::observe(leaf)
                    .is_err()
            );
        }
    }
    let missing = ResolvedLaunchSpec {
        executable: directory.path().join("missing"),
        args: vec!["--acp".into()],
        path_projection: vec![],
    };
    assert_eq!(
        probe_product_version(&missing, Duration::from_millis(50)),
        None
    );
}

/// Registry descriptor 后续读取能见到后台更新；健康事实不因版本成功或失败改变。
#[tokio::test]
async fn background_version_is_eventually_visible_without_health_change() {
    let directory = tempfile::tempdir().unwrap();
    let base = fixture(directory.path());
    let store = StateStore::open(directory.path().join("state"))
        .await
        .unwrap();
    let mut registry = ProviderRegistry::new();
    let mut discovery = DiscoveryResult::direct_for_test(base.to_str().unwrap());
    discovery.launch_spec.executable = base.canonicalize().unwrap();
    register_codebuddy_provider_with_discovery(
        &mut registry,
        store,
        "fixture".into(),
        Ok(discovery),
    )
    .unwrap();
    let id = ProviderId::new("codebuddy".into()).unwrap();
    // 生产 probe 自身仍保持 3 秒上限；测试额外给 CI 线程/进程调度留出余量。
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let descriptor = registry.get_registered(&id).unwrap().descriptor();
        assert_eq!(descriptor.protocol.as_deref(), Some("ACP v1"));
        assert_eq!(registry.health(&id).unwrap(), ProviderHealth::Available);
        if descriptor.version.as_deref() == Some("2.160.0") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "catalog descriptor never updated"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
