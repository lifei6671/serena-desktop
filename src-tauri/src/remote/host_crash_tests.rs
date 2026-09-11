use super::*;
use crate::{
    config::{AppPaths, ManagerConfig},
    serena::SupervisorState,
};

// Invoked only by scripts/test-quick-tunnel-host-crash.ps1, which terminates this
// separate Host process without Drop, remote_stop, cancellation or graceful exit.
#[tokio::test]
#[ignore = "Windows destructive Host test fixture: invoke using the isolated crash test script"]
async fn isolated_quick_tunnel_host() {
    let root = std::path::PathBuf::from(
        std::env::var_os("SERENA_QUICK_CRASH_DIRECTORY")
            .expect("isolated fixture directory required"),
    );
    let paths = AppPaths {
        runtime_directory: root.join("runtime"),
        config_file: root.join("config.json"),
        log_directory: root.join("logs"),
        app_log: root.join("logs/app.log"),
        serena_log: root.join("logs/serena.log"),
    };
    let mut config = ManagerConfig::default();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    config.broker.port = listener.local_addr().unwrap().port();
    drop(listener);
    crate::config::save(&paths.config_file, &config).unwrap();
    let broker = Arc::new(Broker::new(Arc::new(SupervisorState::new(paths).unwrap())));
    let _upstream = crate::serena::remote_fixture::attach(broker.supervisor.clone()).await;
    broker.remote.start(broker.clone()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(190), async {
        loop {
            let snapshot = broker.remote.snapshot();
            if matches!(snapshot.status, Status::Ready | Status::Verifying) {
                let pid = broker.log_snapshot().iter().find_map(|line| line.split_once("Remote cloudflared owned · pid=").and_then(|(_, pid)| pid.trim().parse::<u32>().ok())).unwrap();
                let ready = serde_json::json!({"hostPid":std::process::id(),"cloudflaredPid":pid,"publicOrigin":broker.remote.public_origin(),"stage":"owned_with_public_origin"});
                std::fs::write(root.join("owned.json"), ready.to_string()).unwrap();
                break;
            }
            if matches!(snapshot.status, Status::Error | Status::Disconnected) {
                let error = snapshot.last_error;
                broker.stop().await.unwrap();
                panic!("Quick Tunnel startup failed: {error:?}");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await.expect("Quick Tunnel timeout");
    std::future::pending::<()>().await;
}
