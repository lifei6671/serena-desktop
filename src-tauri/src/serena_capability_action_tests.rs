/// Provider fixture sink 不接收任何 raw process output。
struct IndexTestSink;
impl CapabilityActivitySink for IndexTestSink {}

/// 测试从真实 running Activity 取得 operationId，走与本地调用方相同的取消标识来源。
#[cfg(windows)]
struct IndexCancellationSink(
    tokio::sync::mpsc::UnboundedSender<crate::workspace_capability::CapabilityActivity>,
);
#[cfg(windows)]
impl CapabilityActivitySink for IndexCancellationSink {
    /// 仅转交安全 Activity，不读取 process/root 等私有信息。
    fn publish<'a>(
        &'a self,
        activity: crate::workspace_capability::CapabilityActivity,
    ) -> CapabilityFuture<'a, ()> {
        Box::pin(async move {
            self.0.send(activity).unwrap();
        })
    }
}

/// 通过 Manager 的 operationId 显式取消真实双层 index fixture，确认 Provider future 与后代均收敛。
#[cfg(windows)]
#[tokio::test]
async fn build_index_manager_explicit_cancel_terminates_descendants() {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::WAIT_OBJECT_0,
        System::Threading::{OpenProcess, PROCESS_ALL_ACCESS, WaitForSingleObject},
    };
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("manager-child.pid");
    std::fs::create_dir(directory.path().join(".serena")).unwrap();
    std::fs::write(
        directory.path().join(".serena/project.yml"),
        "languages: [rust]\n",
    )
    .unwrap();
    let target = lease(directory.path().to_path_buf());
    let mut provider = provider(
        installation(
            InstallationState::Standard,
            directory.path().join("managed.exe"),
            "Serena 1.7.0",
        ),
        project_configuration_exists,
    );
    let marker_for_child = marker.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    provider.index_runner = Arc::new(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        // fixture 只替换 executable/argv；仍执行生产 run_index 与 Job-at-creation。
        let mut command = hidden_command(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "serena_capability::tests::index_process_tree_fixture",
                "--nocapture",
            ])
            .env("SERENA_INDEX_TEST_CHILD_MARKER", &marker_for_child);
        run_index(command)
    });
    let provider = Arc::new(provider);
    let registered: Arc<dyn WorkspaceCapabilityProvider> = provider.clone();
    let manager = Arc::new(WorkspaceCapabilityManager::new(Arc::new(
        WorkspaceCapabilityRegistry::new([registered]).unwrap(),
    )));
    let (events, mut received) = tokio::sync::mpsc::unbounded_channel();
    let mut operation = {
        let manager = manager.clone();
        let target = target.clone();
        tokio::spawn(async move {
            manager
                .prepare_action(
                    target,
                    "serena",
                    "build_index",
                    Arc::new(IndexCancellationSink(events)),
                )
                .await
        })
    };
    let running = tokio::time::timeout(Duration::from_secs(5), received.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(running.state, "running");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        tokio::select! {
            result = &mut operation => panic!("index operation completed before child ready: {result:?}"),
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "index fixture readiness timeout"
        );
    }
    let pid = std::fs::read_to_string(marker)
        .unwrap()
        .parse::<u32>()
        .unwrap();
    // SAFETY: PID 来自本 fixture；句柄仅用于取消后等待该后代退出。
    let handle = unsafe { OpenProcess(PROCESS_ALL_ACCESS, 0, pid) };
    assert!(!handle.is_null());
    // SAFETY: OpenProcess 成功后的唯一句柄立即交给 OwnedHandle。
    let child = unsafe { OwnedHandle::from_raw_handle(handle) };
    manager.cancel_action(&running.operation_id).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), operation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        result.unwrap_err().code,
        crate::workspace_capability::WorkspaceCapabilityErrorCode::PrepareFailed
    );
    // SAFETY: child 持有 live process handle，等待具有固定测试上界。
    assert_eq!(
        unsafe { WaitForSingleObject(child.as_raw_handle(), 5_000) },
        WAIT_OBJECT_0
    );
    let terminal = received.recv().await.unwrap();
    assert_eq!(
        (terminal.operation_id, terminal.state, terminal.revision),
        (running.operation_id, "failed", 2)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(provider.runtimes.lock().await.is_empty());
    drop(manager.begin_workspace_remove(&target).await.unwrap());
    manager.shutdown_runtimes().await.unwrap();
}

/// 真实 Serena adapter 与 Manager 组合下，duplicate index 只运行一次且不会额外启动 Runtime。
#[tokio::test]
async fn build_index_manager_duplicates_share_one_operation_without_warming() {
    let directory = tempfile::tempdir().unwrap();
    let target = lease(directory.path().to_path_buf());
    let mut provider = provider(
        installation(
            InstallationState::Standard,
            directory.path().join("managed.exe"),
            "Serena 1.7.0",
        ),
        |_| Ok(true),
    );
    let entered = Arc::new(tokio::sync::Notify::new());
    let (release, receiver) = tokio::sync::oneshot::channel::<()>();
    let receiver = std::sync::Mutex::new(Some(receiver));
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let notify = entered.clone();
    provider.index_runner = Arc::new(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        let receiver = receiver.lock().unwrap().take().unwrap();
        let notify = notify.clone();
        Box::pin(async move {
            notify.notify_one();
            receiver.await.unwrap();
            Ok(())
        })
    });
    let provider = Arc::new(provider);
    let registered: Arc<dyn WorkspaceCapabilityProvider> = provider.clone();
    let manager = Arc::new(WorkspaceCapabilityManager::new(Arc::new(
        WorkspaceCapabilityRegistry::new([registered]).unwrap(),
    )));
    let first = {
        let manager = manager.clone();
        let target = target.clone();
        tokio::spawn(async move {
            manager
                .prepare_action(target, "serena", "build_index", Arc::new(IndexTestSink))
                .await
        })
    };
    entered.notified().await;
    let mut duplicate =
        Box::pin(manager.prepare_action(target, "serena", "build_index", Arc::new(IndexTestSink)));
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut duplicate)
            .await
            .is_err()
    );
    release.send(()).unwrap();
    assert_eq!(first.await.unwrap().unwrap(), duplicate.await.unwrap());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(provider.runtimes.lock().await.is_empty());
}

/// 精确验证当前受管 executable、canonicalRoot argv 与相同 Slot Home authority。
#[tokio::test]
async fn build_index_uses_direct_argv_and_slot_home_exactly_once() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project with spaces");
    std::fs::create_dir_all(root.join(".serena")).unwrap();
    std::fs::write(root.join(".serena/project.yml"), "languages: [rust]\n").unwrap();
    let target = lease(std::fs::canonicalize(&root).unwrap());
    let executable = directory.path().join("managed-serena.exe");
    let mut provider = provider(
        installation(
            InstallationState::Standard,
            executable.clone(),
            "Serena 1.7.0",
        ),
        project_configuration_exists,
    );
    let expected_root = target.canonical_root.clone();
    let expected_home = provider.slot_home(&target);
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    provider.index_runner = Arc::new(move |command| {
        count.fetch_add(1, Ordering::SeqCst);
        assert_eq!(command.get_program(), executable.as_os_str());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![
                std::ffi::OsStr::new("project"),
                std::ffi::OsStr::new("index"),
                expected_root.as_os_str()
            ]
        );
        assert_eq!(
            command
                .get_envs()
                .find(|(name, _)| *name == "SERENA_HOME")
                .unwrap()
                .1,
            Some(expected_home.as_os_str())
        );
        Box::pin(async { Ok(()) })
    });
    let result = provider
        .prepare(
            target.clone(),
            CapabilityPrepareAction {
                action_id: "build_index".into(),
            },
            &IndexTestSink,
        )
        .await
        .unwrap();
    assert_eq!(result.readiness, CapabilityReadinessState::Ready);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(provider.runtimes.lock().await.is_empty());
    let readiness = provider.observe_readiness(target).await.unwrap();
    assert_eq!(readiness.readiness, CapabilityReadinessState::Ready);
    assert!(
        readiness
            .stages
            .iter()
            .filter(|stage| stage.id != "project_configuration")
            .all(|stage| stage.state == CapabilityStageState::Unknown)
    );
}

/// 缺配置、目录伪装配置、安装不兼容与未知 action 都在 index/Home 创建前 fail closed。
#[tokio::test]
async fn build_index_requires_regular_configuration_and_supported_installation() {
    for case in [
        "absent",
        "directory",
        "invalid_installation",
        "prepare",
        "onboarding",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        if case == "directory" {
            std::fs::create_dir_all(root.join(".serena/project.yml")).unwrap();
        }
        let target = lease(root.clone());
        let mut provider = provider(
            installation(
                if case == "invalid_installation" {
                    InstallationState::Invalid
                } else {
                    InstallationState::Standard
                },
                directory.path().join("managed.exe"),
                "Serena 1.7.0",
            ),
            project_configuration_exists,
        );
        provider.index_runner = Arc::new(|_| panic!("invalid prepare must never run index"));
        let action_id = if matches!(case, "prepare" | "onboarding") {
            case
        } else {
            "build_index"
        };
        assert!(
            provider
                .prepare(
                    target.clone(),
                    CapabilityPrepareAction {
                        action_id: action_id.into()
                    },
                    &IndexTestSink
                )
                .await
                .is_err()
        );
        assert!(!provider.slot_home(&target).exists());
        if case != "directory" {
            assert!(!root.join(".serena").exists());
        }
    }
}

/// Future Drop 必须丢弃唯一 index runner；不产生任何 Provider Runtime 或共享 Home。
#[tokio::test]
async fn build_index_future_drop_releases_runner() {
    let directory = tempfile::tempdir().unwrap();
    let target = lease(directory.path().to_path_buf());
    let mut provider = provider(
        installation(
            InstallationState::Standard,
            directory.path().join("managed.exe"),
            "Serena 1.7.0",
        ),
        |_| Ok(true),
    );
    let dropped = Arc::new(AtomicUsize::new(0));
    struct OnDrop(Arc<AtomicUsize>);
    impl Drop for OnDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let observed = dropped.clone();
    provider.index_runner = Arc::new(move |_| {
        let guard = OnDrop(observed.clone());
        Box::pin(async move {
            let _guard = guard;
            std::future::pending().await
        })
    });
    let mut operation = provider.prepare(
        target,
        CapabilityPrepareAction {
            action_id: "build_index".into(),
        },
        &IndexTestSink,
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut operation)
            .await
            .is_err()
    );
    drop(operation);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(provider.runtimes.lock().await.is_empty());
}

/// 仅由本测试的子进程入口触发；直接启动后代以验证 Job tree cleanup。
#[cfg(windows)]
#[test]
fn index_process_tree_fixture() {
    let Some(marker) = std::env::var_os("SERENA_INDEX_TEST_CHILD_MARKER") else {
        return;
    };
    let executable = crate::serena::find_executable("ping.exe").unwrap();
    let mut child = hidden_command(executable)
        .args(["-n", "60", "127.0.0.1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pending = PathBuf::from(&marker).with_extension("pending");
    std::fs::write(&pending, child.id().to_string()).unwrap();
    std::fs::rename(pending, marker).unwrap();
    let _ = child.wait();
}

/// 真实双层进程证明 future timeout/Drop 会关闭创建时 Job，并杀死后代。
#[cfg(windows)]
#[tokio::test]
async fn index_process_timeout_and_drop_terminate_descendants() {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::WAIT_OBJECT_0,
        System::Threading::{OpenProcess, PROCESS_ALL_ACCESS, WaitForSingleObject},
    };
    for cancellation in ["drop", "outer_timeout", "index_deadline"] {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("child.pid");
        let mut command = hidden_command(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "serena_capability::tests::index_process_tree_fixture",
                "--nocapture",
            ])
            .env("SERENA_INDEX_TEST_CHILD_MARKER", &marker);
        let mut operation = run_index(command);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !marker.exists() {
            tokio::select! {
                result = &mut operation => panic!("index fixture exited before child ready: {result:?}"),
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "index fixture readiness timeout"
            );
        }
        let pid = std::fs::read_to_string(&marker)
            .unwrap()
            .parse::<u32>()
            .unwrap();
        // SAFETY: PID 来自本 fixture；马上接管成功 handle，仅用于等待该子进程退出。
        let handle = unsafe { OpenProcess(PROCESS_ALL_ACCESS, 0, pid) };
        assert!(!handle.is_null());
        let child = unsafe { OwnedHandle::from_raw_handle(handle) };
        if cancellation == "outer_timeout" {
            assert!(
                tokio::time::timeout(Duration::from_millis(20), operation)
                    .await
                    .is_err()
            );
        } else if cancellation == "index_deadline" {
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(91)).await;
            assert!(operation.await.is_err());
            tokio::time::resume();
        } else {
            drop(operation);
        }
        // SAFETY: handle 在等待过程中由 child 保持存活，5 秒为测试失败上界。
        assert_eq!(
            unsafe { WaitForSingleObject(child.as_raw_handle(), 5_000) },
            WAIT_OBJECT_0
        );
    }
}
