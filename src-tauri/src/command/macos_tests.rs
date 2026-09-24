use super::*;
use crate::config::{self, AppPaths, ManagerConfig, Workspace};

/// 建立真实 Workspace lease、StateStore 和 CommandService。
async fn fixture() -> (tempfile::TempDir, CommandService, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let paths = AppPaths {
        runtime_directory: directory.path().join("runtime"),
        config_file: directory.path().join("config.json"),
        log_directory: directory.path().join("logs"),
        app_log: directory.path().join("logs/app.log"),
        serena_log: directory.path().join("logs/serena.log"),
    };
    config::save(
        &paths.config_file,
        &ManagerConfig {
            workspaces: vec![Workspace {
                id: "command-test".into(),
                name: "Command Test".into(),
                root: workspace.clone(),
                generation: 1,
            }],
            ..ManagerConfig::default()
        },
    )
    .unwrap();
    let supervisor = Arc::new(SupervisorState::new(paths).unwrap());
    let store = StateStore::open(directory.path().join("state"))
        .await
        .unwrap();
    let service = CommandService::new(store, supervisor).await.unwrap();
    (directory, service, workspace)
}

/// 提取成功 Run，同时让失败类别在断言信息中可见。
fn run(envelope: CommandEnvelope) -> CommandRunView {
    match envelope {
        CommandEnvelope::Success { data, .. } => match *data {
            CommandData::Run { command_run } => command_run,
            other => panic!("unexpected command data: {other:?}"),
        },
        other => panic!("unexpected command result: {other:?}"),
    }
}

/// 启动测试命令时保持全部公共请求字段通过真实 service 入口。
async fn start(
    service: &CommandService,
    key: &str,
    spec: CommandSpec,
    cwd: Option<&str>,
    mode: ExecutionMode,
    timeout_ms: u64,
) -> CommandRunView {
    run(service
        .execute(ExecuteRequest::Start {
            workspace_id: "command-test".into(),
            request_key: key.into(),
            work_run_id: None,
            spec,
            relative_cwd: cwd.map(str::to_owned),
            env: BTreeMap::new(),
            timeout_ms: Some(timeout_ms),
            execution_mode: Some(mode),
            yield_time_ms: Some(5_000),
        })
        .await)
}

/// 读取保留的两个输出流。
async fn output(service: &CommandService, id: String) -> CommandOutputView {
    match service
        .query(QueryRequest::Output {
            command_run_id: id,
            stdout_cursor: Some(0),
            stderr_cursor: Some(0),
            max_output_bytes: None,
        })
        .await
    {
        CommandEnvelope::Success { data, .. } => match *data {
            CommandData::Output { output } => output,
            other => panic!("unexpected data: {other:?}"),
        },
        other => panic!("unexpected output: {other:?}"),
    }
}

/// Shell 选择由账户记录决定，且 -c 是唯一额外调用参数。
#[test]
fn shell_uses_account_login_shell_or_sh_fallback() {
    let expected = macos_launcher::account()
        .map(|(shell, _)| shell)
        .filter(|shell| macos_launcher::executable_file(shell))
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    let command_path = macos_launcher::command_path(&BTreeMap::new()).unwrap();
    let (shell, args) = macos_launcher::invocation(
        &CommandSpec::Shell {
            command: "printf one; printf two".into(),
        },
        &command_path,
    )
    .unwrap();
    assert_eq!(shell, expected);
    assert_eq!(args, ["-c", "printf one; printf two"]);
}

/// 显式 PATH 的相对项在 Command 入口返回稳定环境错误。
#[tokio::test]
async fn explicit_relative_path_is_rejected_by_command_entry() {
    let (_directory, service, _) = fixture().await;
    let result = service
        .execute(ExecuteRequest::Start {
            workspace_id: "command-test".into(),
            request_key: "relative-path".into(),
            work_run_id: None,
            spec: CommandSpec::Process {
                executable: "pwd".into(),
                args: vec![],
            },
            relative_cwd: None,
            env: BTreeMap::from([("PATH".into(), "relative:/usr/bin".into())]),
            timeout_ms: None,
            execution_mode: None,
            yield_time_ms: None,
        })
        .await;
    match result {
        CommandEnvelope::Failure { error, .. } => assert_eq!(error.code, "COMMAND_ENV_INVALID"),
        other => panic!("unexpected command result: {other:?}"),
    }
    service.shutdown().await.unwrap();
}

/// Process 通过 PATH 命中的 symlink proxy 启动时必须保留入口名，覆盖 rustup/corepack 多调用语义。
#[tokio::test]
async fn process_preserves_path_proxy_entry_end_to_end() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let (directory, service, _) = fixture().await;
    let bin = directory.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let target = bin.join("multicall");
    let proxy = bin.join("cargo");
    std::fs::write(&target, "#!/bin/sh\nprintf '%s|%s' \"$0\" \"$1\"\n").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&target, &proxy).unwrap();

    let result = run(service
        .execute(ExecuteRequest::Start {
            workspace_id: "command-test".into(),
            request_key: "proxy-entry".into(),
            work_run_id: None,
            spec: CommandSpec::Process {
                executable: "cargo".into(),
                args: vec!["--probe".into()],
            },
            relative_cwd: None,
            env: BTreeMap::from([("PATH".into(), format!("{}:/usr/bin:/bin", bin.display()))]),
            timeout_ms: Some(10_000),
            execution_mode: Some(ExecutionMode::Sync),
            yield_time_ms: Some(5_000),
        })
        .await);
    assert_eq!(result.status, "completed");
    assert_eq!(result.exit_code, Some(0));
    let contents = output(&service, result.command_run_id).await;
    assert_eq!(contents.stdout.text, format!("{}|--probe", proxy.display()));
    service.shutdown().await.unwrap();
}

/// 原生命令输出、失败 exit code、工作目录及请求幂等均经过真实进程。
#[tokio::test]
async fn process_executes_and_keeps_cwd_output_and_request_key() {
    let (_directory, service, workspace) = fixture().await;
    std::fs::create_dir(workspace.join("sub")).unwrap();
    let done = start(
        &service,
        "pwd",
        CommandSpec::Process {
            executable: "pwd".into(),
            args: vec![],
        },
        Some("sub"),
        ExecutionMode::Auto,
        10_000,
    )
    .await;
    assert_eq!(done.status, "completed");
    assert_eq!(done.exit_code, Some(0));
    assert_eq!(done.runtime_platform, "macos");
    let contents = output(&service, done.command_run_id.clone()).await;
    assert_eq!(
        contents.stdout.text.trim(),
        workspace
            .join("sub")
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
    );
    assert_eq!(contents.stderr.total_bytes, 0);
    let again = start(
        &service,
        "pwd",
        CommandSpec::Process {
            executable: "pwd".into(),
            args: vec![],
        },
        Some("sub"),
        ExecutionMode::Auto,
        10_000,
    )
    .await;
    assert_eq!(again.command_run_id, done.command_run_id);

    let failed = start(
        &service,
        "ls-missing",
        CommandSpec::Process {
            executable: "ls".into(),
            args: vec!["definitely-missing-command-test-entry".into()],
        },
        None,
        ExecutionMode::Auto,
        10_000,
    )
    .await;
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.exit_code, Some(1));
    assert!(
        output(&service, failed.command_run_id)
            .await
            .stderr
            .total_bytes
            > 0
    );

    let escaped = service
        .execute(ExecuteRequest::Start {
            workspace_id: "command-test".into(),
            request_key: "escaped".into(),
            work_run_id: None,
            spec: CommandSpec::Process {
                executable: "pwd".into(),
                args: vec![],
            },
            relative_cwd: Some("../".into()),
            env: BTreeMap::new(),
            timeout_ms: None,
            execution_mode: None,
            yield_time_ms: None,
        })
        .await;
    assert!(matches!(escaped, CommandEnvelope::Failure { .. }));
    service.shutdown().await.unwrap();
}

/// Shell 组合命令同时覆盖 stdout、stderr 和显式 exit code。
#[tokio::test]
async fn shell_executes_combined_command() {
    let (_directory, service, _) = fixture().await;
    let done = start(
        &service,
        "combined",
        CommandSpec::Shell {
            command: "printf alpha; printf beta >&2; exit 7".into(),
        },
        None,
        ExecutionMode::Auto,
        10_000,
    )
    .await;
    assert_eq!(done.status, "failed");
    assert_eq!(done.exit_code, Some(7));
    let contents = output(&service, done.command_run_id).await;
    assert_eq!(contents.stdout.text, "alpha");
    assert_eq!(contents.stderr.text, "beta");
    service.shutdown().await.unwrap();
}

/// 异步观察、取消和超时都要求子进程回收及组清空。
#[tokio::test]
async fn async_observe_cancel_timeout_and_descendant_group_cleanup() {
    let (_directory, service, _) = fixture().await;
    let running = start(
        &service,
        "observe",
        CommandSpec::Shell {
            command: "printf ready; /bin/sleep 1; printf done".into(),
        },
        None,
        ExecutionMode::Async,
        10_000,
    )
    .await;
    assert_eq!(running.status, "running");
    let mut revision = running.revision;
    loop {
        let observed = service
            .query(QueryRequest::Observe {
                command_run_id: running.command_run_id.clone(),
                known_revision: Some(revision),
                wait_ms: Some(5_000),
            })
            .await;
        let view = match observed {
            CommandEnvelope::Success { data, .. } => match *data {
                CommandData::Observation { observation } => observation.command_run,
                other => panic!("unexpected data: {other:?}"),
            },
            other => panic!("unexpected observation: {other:?}"),
        };
        if view.status == "completed" {
            break;
        }
        assert_eq!(view.status, "running");
        revision = view.revision;
    }
    assert_eq!(
        output(&service, running.command_run_id).await.stdout.text,
        "readydone"
    );

    let running = start(
        &service,
        "cancel-tree",
        CommandSpec::Shell {
            command: "/bin/sleep 30 & wait".into(),
        },
        None,
        ExecutionMode::Async,
        60_000,
    )
    .await;
    let control = service.live.lock().unwrap()[&running.command_run_id]
        .control
        .clone();
    let cancelled = run(service
        .execute(ExecuteRequest::Cancel {
            command_run_id: running.command_run_id,
        })
        .await);
    assert_eq!(cancelled.status, "cancelled");
    assert!(control.complete_evidence().unwrap());

    let running = start(
        &service,
        "timeout-tree",
        CommandSpec::Shell {
            command: "/bin/sleep 30 & wait".into(),
        },
        None,
        ExecutionMode::Async,
        150,
    )
    .await;
    let control = service.live.lock().unwrap()[&running.command_run_id]
        .control
        .clone();
    let deadline = Instant::now() + Duration::from_secs(8);
    let terminal = loop {
        let view = service.view(&running.command_run_id).await.unwrap();
        if is_terminal(&view.status) {
            break view;
        }
        assert!(Instant::now() < deadline, "timeout did not reach terminal");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(terminal.status, "failed");
    assert!(terminal.timed_out);
    assert!(control.complete_evidence().unwrap());
    service.shutdown().await.unwrap();
}

/// Host restart 只允许 Windows Job 记录自动收敛，macOS 进程组证据不足保持 unknown。
#[tokio::test]
async fn recovery_requires_matching_windows_job_evidence() {
    let (_directory, service, workspace) = fixture().await;
    for (id, platform, containment) in [
        ("old-win", "windows", "job_at_creation"),
        ("old-mac", "macos", "process_group"),
    ] {
        service
            .store
            .create_command_run(
                id.into(),
                CreateCommandRunInput {
                    request_key: id.into(),
                    request_hash: id.into(),
                    workspace_id: "command-test".into(),
                    canonical_workspace_root: workspace
                        .canonicalize()
                        .unwrap()
                        .display()
                        .to_string(),
                    workspace_generation: 1,
                    work_run_id: None,
                    mode: "process".into(),
                    relative_cwd: ".".into(),
                    execution_mode: "async".into(),
                    timeout_ms: 10_000,
                    runtime_platform: platform.into(),
                    containment_type: containment.into(),
                },
                now_millis(),
            )
            .await
            .unwrap();
    }
    assert_eq!(
        service
            .store
            .recover_command_runs(true, now_millis())
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        service
            .store
            .command_run("old-win".into())
            .await
            .unwrap()
            .unwrap()
            .status,
        "interrupted"
    );
    assert_eq!(
        service
            .store
            .command_run("old-mac".into())
            .await
            .unwrap()
            .unwrap()
            .status,
        "unknown"
    );
}

/// leader 自然退出后，短命后代未离组前不得写 terminal Receipt。
#[tokio::test]
async fn natural_parent_exit_waits_for_descendant_group_to_empty() {
    let (_directory, service, _) = fixture().await;
    let done = start(
        &service,
        "natural-descendant",
        CommandSpec::Shell {
            command: "/bin/sleep 1 &".into(),
        },
        None,
        ExecutionMode::Auto,
        10_000,
    )
    .await;
    assert_eq!(done.status, "completed");
    assert!(
        service.live.lock().unwrap()[&done.command_run_id]
            .control
            .complete_evidence()
            .unwrap()
    );
    service.shutdown().await.unwrap();
}

/// 后代超过旧 3 秒 seal grace 时，CommandRun 与 Workspace guard 均保持 live。
#[tokio::test]
async fn long_lived_descendant_keeps_run_and_workspace_owned_until_group_empty() {
    let (_directory, service, _) = fixture().await;
    let product = Arc::new(crate::agent::product::AgentProductService::new(
        service.store.clone(),
    ));
    let running = start(
        &service,
        "long-descendant",
        CommandSpec::Shell {
            command: "/bin/sleep 8 &".into(),
        },
        None,
        ExecutionMode::Async,
        15_000,
    )
    .await;
    assert_eq!(running.status, "running");
    let live = service.live.lock().unwrap()[&running.command_run_id].clone();
    tokio::time::sleep(Duration::from_millis(3_300)).await;
    let still_running = service.view(&running.command_run_id).await.unwrap();
    assert!(!is_terminal(&still_running.status), "{still_running:?}");
    assert_eq!(live.terminal_at.load(Ordering::Acquire), 0);
    assert_eq!(service.permits.available_permits(), MAX_CONCURRENT_RUNS - 1);
    assert!(!live.control.complete_evidence().unwrap());
    let supervisor = service.supervisor.clone();
    let remove_product = product.clone();
    let blocked_remove = tokio::task::spawn_blocking(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(supervisor.remove_workspace_coordinated(&remove_product, "command-test"))
    })
    .await
    .unwrap();
    assert_eq!(
        blocked_remove,
        Err(crate::workspace_registry::WORKSPACE_IN_USE.into())
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let view = service.view(&running.command_run_id).await.unwrap();
        if is_terminal(&view.status) && service.permits.available_permits() == MAX_CONCURRENT_RUNS {
            assert_eq!(view.status, "completed");
            assert!(live.control.complete_evidence().unwrap());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "group did not close after descendant exit"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    service
        .supervisor
        .remove_workspace_coordinated(&product, "command-test")
        .await
        .unwrap();
    service.shutdown().await.unwrap();
}

/// leader 已退出时取消不能凭旧 PGID kill，也不能提前释放 Workspace ownership。
#[tokio::test]
async fn cancel_after_parent_exit_stays_unresolved_until_group_empty() {
    let (_directory, service, _) = fixture().await;
    let running = start(
        &service,
        "cancel-after-exit",
        CommandSpec::Shell {
            command: "/bin/sleep 4 &".into(),
        },
        None,
        ExecutionMode::Async,
        10_000,
    )
    .await;
    let live = service.live.lock().unwrap()[&running.command_run_id].clone();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!live.control.complete_evidence().unwrap());
    let cancelled = service
        .execute(ExecuteRequest::Cancel {
            command_run_id: running.command_run_id.clone(),
        })
        .await;
    assert!(matches!(cancelled, CommandEnvelope::Failure { .. }));
    let unresolved = service.view(&running.command_run_id).await.unwrap();
    assert_eq!(unresolved.status, "cancelling");
    assert_eq!(live.terminal_at.load(Ordering::Acquire), 0);
    assert_eq!(service.permits.available_permits(), MAX_CONCURRENT_RUNS - 1);

    let deadline = Instant::now() + Duration::from_secs(7);
    loop {
        let view = service.view(&running.command_run_id).await.unwrap();
        if is_terminal(&view.status) {
            assert_eq!(view.status, "unknown");
            assert!(live.control.complete_evidence().unwrap());
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    service.shutdown().await.unwrap();
}

/// parent 自然退出后超时仍保持 unresolved，待组空才形成 unknown Receipt。
#[tokio::test]
async fn timeout_after_parent_exit_stays_unresolved_until_group_empty() {
    let (_directory, service, _) = fixture().await;
    let running = start(
        &service,
        "timeout-after-exit",
        CommandSpec::Shell {
            command: "/bin/sleep 4 &".into(),
        },
        None,
        ExecutionMode::Async,
        200,
    )
    .await;
    let live = service.live.lock().unwrap()[&running.command_run_id].clone();
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    let unresolved = service.view(&running.command_run_id).await.unwrap();
    assert_eq!(unresolved.status, "cancelling");
    assert_eq!(live.terminal_at.load(Ordering::Acquire), 0);
    assert_eq!(service.permits.available_permits(), MAX_CONCURRENT_RUNS - 1);
    assert!(!live.control.complete_evidence().unwrap());

    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let view = service.view(&running.command_run_id).await.unwrap();
        if is_terminal(&view.status) {
            assert_eq!(view.status, "unknown");
            assert!(view.timed_out);
            assert!(live.control.complete_evidence().unwrap());
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    service.shutdown().await.unwrap();
}
