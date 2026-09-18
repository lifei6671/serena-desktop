/// 独立 capability sink 记录真实发布序列，不接触 Agent Activity。
#[derive(Default)]
struct RecordingActionSink(Mutex<Vec<CapabilityActivity>>);
impl CapabilityActivitySink for RecordingActionSink {
    fn publish<'a>(&'a self, activity: CapabilityActivity) -> CapabilityFuture<'a, ()> {
        Box::pin(async move {
            lock_unpoisoned(&self.0).push(activity);
        })
    }
}

/// 用通知精确阻塞 terminal 发布，暴露结果已决但 operation 尚未移除的取消窗口。
#[derive(Default)]
struct BlockedTerminalActionSink {
    events: Mutex<Vec<CapabilityActivity>>,
    terminal_entered: Notify,
    release_terminal: Notify,
}

impl CapabilityActivitySink for BlockedTerminalActionSink {
    /// terminal 只有测试明确放行后才被记录为已发布；不依赖 sleep 或调度时序。
    fn publish<'a>(&'a self, activity: CapabilityActivity) -> CapabilityFuture<'a, ()> {
        Box::pin(async move {
            if activity.revision == 2 {
                self.terminal_entered.notify_one();
                self.release_terminal.notified().await;
            }
            lock_unpoisoned(&self.events).push(activity);
        })
    }
}

/// candidate 成功后关闭取消窗口，即使 terminal sink 阻塞，也不能返回假成功取消回执。
#[tokio::test]
async fn cancel_after_result_decided_rejects_while_terminal_publication_is_pending() {
    let provider = action_provider(CapabilityActionExecution::ProviderPrepare, false);
    let release = provider.enqueue_prepare();
    let manager = runtime_manager(provider.clone());
    let sink = Arc::new(BlockedTerminalActionSink::default());
    let target = lease("terminal-race", 1);
    let first = {
        let manager = manager.clone();
        let sink = sink.clone();
        let target = target.clone();
        tokio::spawn(async move {
            manager
                .prepare_action(target, "third-action", "prepare_index", sink)
                .await
        })
    };
    provider.prepare_entered.notified().await;
    let operation_id = lock_unpoisoned(&sink.events)[0].operation_id.clone();
    let mut duplicate = Box::pin(manager.prepare_action(
        target.clone(),
        "third-action",
        "prepare_index",
        sink.clone(),
    ));
    // 精确 poll 将第二个 caller 登记到同一 flight，再放行 Provider 的成功返回。
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(duplicate.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    release.send(CallPlan::Success).unwrap();
    sink.terminal_entered.notified().await;
    {
        let table = lock_unpoisoned(&manager.runtime_slots);
        let flight = table
            .operations
            .values()
            .find(|flight| flight.operation_id == operation_id)
            .unwrap();
        assert!(
            flight.completion.borrow().is_none(),
            "completion must follow terminal Activity"
        );
    }
    assert_eq!(lock_unpoisoned(&sink.events).len(), 1);
    assert!(!first.is_finished());
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(duplicate.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    for _ in 0..2 {
        assert_eq!(
            manager.cancel_action(&operation_id).unwrap_err().code,
            WorkspaceCapabilityErrorCode::NotFound
        );
    }
    assert!(
        matches!(manager.begin_workspace_remove(&target).await, Err(error) if error.code == WorkspaceCapabilityErrorCode::Busy)
    );
    sink.release_terminal.notify_one();
    let result = first.await.unwrap().unwrap();
    assert_eq!(result.operation_id, operation_id);
    assert_eq!(duplicate.await.unwrap(), result);
    assert_eq!(provider.prepares.load(Ordering::SeqCst), 1);
    let events = lock_unpoisoned(&sink.events).clone();
    assert_eq!(events.len(), 2);
    assert_eq!((events[1].state, events[1].revision), ("succeeded", 2));
    assert_eq!(events[1].operation_id, operation_id);
    drop(manager.begin_workspace_remove(&target).await.unwrap());
    assert_eq!(
        manager.cancel_action(&operation_id).unwrap_err().code,
        WorkspaceCapabilityErrorCode::NotFound
    );
}

/// 用任意第三方 ID 验证 Core 没有 Serena 分支。
fn action_provider(execution: CapabilityActionExecution, warm_runtime: bool) -> Arc<FakeProvider> {
    let mut descriptor = descriptor("third-action");
    descriptor.runtime_model = CapabilityRuntimeModel::WorkspaceScopedProcess;
    descriptor.action_descriptors[0].execution = execution;
    descriptor.action_descriptors[0].warm_runtime = warm_runtime;
    Arc::new(FakeProvider::with_descriptor(descriptor))
}

/// duplicate 调用只有一次 Provider action，且删除/Tool 都受 operation claim 保护。
#[tokio::test]
async fn explicit_action_single_flight_safe_activity_and_remove_claim() {
    let provider = action_provider(CapabilityActionExecution::ProviderPrepare, false);
    let release = provider.enqueue_prepare();
    let manager = runtime_manager(Arc::clone(&provider));
    let sink = Arc::new(RecordingActionSink::default());
    let target = lease("action-a", 3);
    let first = {
        let manager = Arc::clone(&manager);
        let sink = sink.clone();
        let target = target.clone();
        tokio::spawn(async move {
            manager
                .prepare_action(target, "third-action", "prepare_index", sink)
                .await
        })
    };
    provider.prepare_entered.notified().await;
    assert!(
        matches!(manager.begin_workspace_remove(&target).await, Err(error) if error.code == WorkspaceCapabilityErrorCode::Busy)
    );
    assert!(
        matches!(manager.acquire_runtime("third-action", target.clone()).await, Err(error) if error.code == WorkspaceCapabilityErrorCode::Busy)
    );
    let mut duplicate = Box::pin(manager.prepare_action(
        target.clone(),
        "third-action",
        "prepare_index",
        sink.clone(),
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut duplicate)
            .await
            .is_err()
    );
    release.send(CallPlan::Success).unwrap();
    let result = first.await.unwrap().unwrap();
    assert_eq!(duplicate.await.unwrap(), result);
    assert_eq!(
        provider.prepares.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(provider.starts.load(std::sync::atomic::Ordering::SeqCst), 0);
    drop(manager.begin_workspace_remove(&target).await.unwrap());
    let events = lock_unpoisoned(&sink.0);
    assert_eq!(events.len(), 2);
    assert_eq!((events[0].revision, events[1].revision), (1, 2));
    assert_eq!((events[0].state, events[1].state), ("running", "succeeded"));
    assert!(
        events
            .iter()
            .all(|event| event.operation_id == result.operation_id)
    );
    let value = serde_json::to_value(&*events).unwrap();
    for event in value.as_array().unwrap() {
        let object = event.as_object().unwrap();
        assert_eq!(object.len(), 8);
        for field in [
            "operationId",
            "workspaceId",
            "providerId",
            "actionId",
            "stageCode",
            "state",
            "revision",
            "messageCode",
        ] {
            assert!(object.contains_key(field));
        }
    }
    let serialized = value.to_string();
    for forbidden in [
        "server-resolved",
        "SERENA_HOME",
        "stdout",
        "stderr",
        "command",
        "argv",
        "PID",
        "port",
        "raw error",
    ] {
        assert!(!serialized.contains(forbidden));
    }
}

/// prepare 复用原 acquire/start single-flight，成功后保留 idle Runtime。
#[tokio::test]
async fn explicit_action_ensure_runtime_warms_without_provider_prepare() {
    let provider = action_provider(CapabilityActionExecution::ManagerEnsureRuntime, true);
    let manager = runtime_manager(Arc::clone(&provider));
    let target = lease("warm", 1);
    let result = manager
        .prepare_action(
            target.clone(),
            "third-action",
            "prepare_index",
            Arc::new(RecordingActionSink::default()),
        )
        .await
        .unwrap();
    assert_eq!(result.readiness, CapabilityReadinessState::Ready);
    let guard = manager
        .acquire_runtime("third-action", target)
        .await
        .unwrap();
    assert_eq!(provider.starts.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        provider.prepares.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert_eq!(provider.stops.load(std::sync::atomic::Ordering::SeqCst), 0);
    drop(guard);
    manager.shutdown_runtimes().await.unwrap();
}

/// in-flight/startup/pending 均拒绝 index；idle 必须先 stop，stop failure 保留容量。
#[tokio::test]
async fn explicit_action_busy_idle_stop_and_stop_failure() {
    for stop_fails in [false, true] {
        let provider = action_provider(CapabilityActionExecution::ProviderPrepare, false);
        let manager = runtime_manager(Arc::clone(&provider));
        let target = lease("busy", 1);
        let release = provider.enqueue_start();
        let acquire = {
            let manager = manager.clone();
            let target = target.clone();
            tokio::spawn(async move { manager.acquire_runtime("third-action", target).await })
        };
        provider.start_entered.notified().await;
        let sink = Arc::new(RecordingActionSink::default());
        assert_eq!(
            manager
                .prepare_action(
                    target.clone(),
                    "third-action",
                    "prepare_index",
                    sink.clone()
                )
                .await
                .unwrap_err()
                .code,
            WorkspaceCapabilityErrorCode::Busy
        );
        release.send(StartPlan::Success).unwrap();
        let guard = acquire.await.unwrap().unwrap();
        let mut pending = Box::pin(manager.acquire_runtime("third-action", target.clone()));
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut pending)
                .await
                .is_err()
        );
        assert_eq!(
            manager
                .prepare_action(
                    target.clone(),
                    "third-action",
                    "prepare_index",
                    sink.clone()
                )
                .await
                .unwrap_err()
                .code,
            WorkspaceCapabilityErrorCode::Busy
        );
        drop(guard);
        assert_eq!(
            manager
                .prepare_action(
                    target.clone(),
                    "third-action",
                    "prepare_index",
                    sink.clone()
                )
                .await
                .unwrap_err()
                .code,
            WorkspaceCapabilityErrorCode::Busy
        );
        drop(pending);
        if stop_fails {
            provider.fail_stop();
        }
        let result = manager
            .prepare_action(target.clone(), "third-action", "prepare_index", sink)
            .await;
        assert_eq!(provider.stops.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(provider.starts.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            provider.prepares.load(std::sync::atomic::Ordering::SeqCst),
            usize::from(!stop_fails)
        );
        if stop_fails {
            assert_eq!(
                result.unwrap_err().code,
                WorkspaceCapabilityErrorCode::StopFailed
            );
            let table = lock_unpoisoned(&manager.runtime_slots);
            assert_eq!(
                WorkspaceCapabilityManager::allocated_capacity(
                    &table,
                    &WorkspaceCapabilityProviderId::new("third-action")
                ),
                1
            );
            assert!(
                lock_unpoisoned(&table.slots.values().next().unwrap().state)
                    .runtime
                    .is_some()
            );
        } else {
            assert!(result.is_ok());
        }
    }
}

/// Caller cancellation 不取消共享操作；失败后释放 claim，其他 Workspace 可独立执行。
#[tokio::test]
async fn explicit_action_cancel_failure_isolation_and_unknown_ids() {
    let provider = action_provider(CapabilityActionExecution::ProviderPrepare, false);
    let release = provider.enqueue_prepare();
    let manager = runtime_manager(provider.clone());
    let sink = Arc::new(RecordingActionSink::default());
    for (provider_id, action_id) in [("missing", "prepare_index"), ("third-action", "onboarding")] {
        assert_eq!(
            manager
                .prepare_action(lease("a", 1), provider_id, action_id, sink.clone())
                .await
                .unwrap_err()
                .code,
            WorkspaceCapabilityErrorCode::NotFound
        );
    }
    let first = {
        let manager = manager.clone();
        let sink = sink.clone();
        tokio::spawn(async move {
            manager
                .prepare_action(lease("a", 1), "third-action", "prepare_index", sink)
                .await
        })
    };
    provider.prepare_entered.notified().await;
    first.abort();
    let _ = first.await;
    let mut join = Box::pin(manager.prepare_action(
        lease("a", 1),
        "third-action",
        "prepare_index",
        sink.clone(),
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut join)
            .await
            .is_err()
    );
    assert!(
        manager
            .prepare_action(lease("b", 1), "third-action", "prepare_index", sink.clone())
            .await
            .is_ok()
    );
    release.send(CallPlan::Failure).unwrap();
    assert_eq!(
        join.await.unwrap_err().code,
        WorkspaceCapabilityErrorCode::PrepareFailed
    );
    drop(
        manager
            .begin_workspace_remove(&lease("a", 1))
            .await
            .unwrap(),
    );
    assert!(
        lock_unpoisoned(&sink.0)
            .iter()
            .any(|event| event.workspace_id == "a" && event.state == "failed")
    );
}

/// 使用虚拟时间证明 timeout 丢弃 Provider future 并释放 operation claim。
#[tokio::test(start_paused = true)]
async fn explicit_action_timeout_drops_provider_future_and_claim() {
    let provider = action_provider(CapabilityActionExecution::ProviderPrepare, false);
    let mut release = provider.enqueue_prepare();
    let manager = runtime_manager(provider.clone());
    let sink = Arc::new(RecordingActionSink::default());
    let result = manager
        .prepare_action(
            lease("timeout", 1),
            "third-action",
            "prepare_index",
            sink.clone(),
        )
        .await;
    assert_eq!(
        result.unwrap_err().code,
        WorkspaceCapabilityErrorCode::PrepareFailed
    );
    release.closed().await;
    assert_eq!(
        provider.prepares.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    drop(
        manager
            .begin_workspace_remove(&lease("timeout", 1))
            .await
            .unwrap(),
    );
    let events = lock_unpoisoned(&sink.0);
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].state, "failed");
}

/// 真实本地 handler 用 explicit ID 解析 Lease；失败不改变 Registry/selection，Remove 与 operation 协调。
#[tokio::test]
async fn local_capability_action_resolves_lease_and_coordinates_workspace_remove() {
    for explicit_cancel in [false, true] {
        use crate::{
            agent::{product::AgentProductService, store::StateStore},
            config::{self, AppPaths, ManagerConfig, Workspace},
            serena::SupervisorState,
            workspace_registry::{WORKSPACE_IN_USE, WORKSPACE_NOT_FOUND, WorkspaceRegistry},
        };
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs/app.log"),
            serena_log: directory.path().join("logs/serena.log"),
        };
        let workspaces = ["target", "selected"]
            .into_iter()
            .map(|id| {
                let root = directory.path().join(id);
                std::fs::create_dir(&root).unwrap();
                Workspace {
                    id: id.into(),
                    name: id.into(),
                    root: std::fs::canonicalize(root).unwrap(),
                    generation: 7,
                }
            })
            .collect::<Vec<_>>();
        config::save(
            &paths.config_file,
            &ManagerConfig {
                workspaces: workspaces.clone(),
                desktop_selected_workspace_id: Some("selected".into()),
                ..ManagerConfig::default()
            },
        )
        .unwrap();
        let provider = action_provider(CapabilityActionExecution::ProviderPrepare, false);
        let manager = runtime_manager(provider.clone());
        let mut supervisor = SupervisorState::new(paths.clone()).unwrap();
        supervisor.replace_workspace_capability_manager_for_test(manager);
        let supervisor = Arc::new(supervisor);
        let sink = Arc::new(RecordingActionSink::default());
        let before = WorkspaceRegistry::new(&supervisor).list();
        let bytes = std::fs::read(&paths.config_file).unwrap();
        assert_eq!(
            crate::commands::prepare_workspace_capability(
                &supervisor,
                "missing",
                "third-action",
                "prepare_index",
                sink.clone()
            )
            .await
            .unwrap_err(),
            WORKSPACE_NOT_FOUND
        );
        let release = provider.enqueue_prepare();
        let operation = {
            let supervisor = supervisor.clone();
            let sink = sink.clone();
            tokio::spawn(async move {
                crate::commands::prepare_workspace_capability(
                    &supervisor,
                    "target",
                    "third-action",
                    "prepare_index",
                    sink,
                )
                .await
            })
        };
        provider.prepare_entered.notified().await;
        let leases = lock_unpoisoned(&provider.prepared_leases).clone();
        assert_eq!(
            leases,
            vec![WorkspaceLease {
                workspace_id: "target".into(),
                canonical_root: workspaces[0].root.clone(),
                generation: 7
            }]
        );
        let product = AgentProductService::new(
            StateStore::open(directory.path().join("state"))
                .await
                .unwrap(),
        );
        assert_eq!(
            crate::commands::remove_workspace(&supervisor, &product, "target").await,
            Err(WORKSPACE_IN_USE.into())
        );
        let operation_id = lock_unpoisoned(&sink.0)[0].operation_id.clone();
        if explicit_cancel {
            assert_eq!(
                crate::commands::cancel_workspace_capability(&supervisor, "cap-op-unknown"),
                Err("WORKSPACE_CAPABILITY_NOT_FOUND".into())
            );
            crate::commands::cancel_workspace_capability(&supervisor, &operation_id).unwrap();
            crate::commands::cancel_workspace_capability(&supervisor, &operation_id).unwrap();
        } else {
            release.send(CallPlan::Failure).unwrap();
        }
        assert_eq!(
            operation.await.unwrap().unwrap_err(),
            "WORKSPACE_CAPABILITY_PREPARE_FAILED"
        );
        assert_eq!(WorkspaceRegistry::new(&supervisor).list(), before);
        assert_eq!(std::fs::read(&paths.config_file).unwrap(), bytes);
        assert_eq!(
            crate::commands::cancel_workspace_capability(&supervisor, &operation_id),
            Err("WORKSPACE_CAPABILITY_NOT_FOUND".into())
        );
        assert!(
            crate::commands::remove_workspace(&supervisor, &product, "target")
                .await
                .is_ok()
        );
        assert!(workspaces[0].root.exists());
        assert_eq!(
            supervisor
                .workspace_registry_config()
                .desktop_selected_workspace_id
                .as_deref(),
            Some("selected")
        );
    }
}

/// 显式 cancel 取消同一 flight 的所有 joiner，并先 drop Provider future 再释放 claim。
#[tokio::test]
async fn explicit_cancel_shares_failure_drops_provider_and_preserves_safe_activity() {
    let provider = action_provider(CapabilityActionExecution::ProviderPrepare, false);
    let release = provider.enqueue_prepare();
    let manager = runtime_manager(provider.clone());
    let sink = Arc::new(RecordingActionSink::default());
    let target = lease("explicit-cancel", 1);
    let first = {
        let manager = manager.clone();
        let sink = sink.clone();
        let target = target.clone();
        tokio::spawn(async move {
            manager
                .prepare_action(target, "third-action", "prepare_index", sink)
                .await
        })
    };
    provider.prepare_entered.notified().await;
    let operation_id = lock_unpoisoned(&sink.0)[0].operation_id.clone();
    let mut duplicate = Box::pin(manager.prepare_action(
        target.clone(),
        "third-action",
        "prepare_index",
        sink.clone(),
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut duplicate)
            .await
            .is_err()
    );
    assert!(
        matches!(manager.begin_workspace_remove(&target).await, Err(error) if error.code == WorkspaceCapabilityErrorCode::Busy)
    );
    assert_eq!(
        manager.cancel_action("cap-op-unknown").unwrap_err().code,
        WorkspaceCapabilityErrorCode::NotFound
    );
    manager.cancel_action(&operation_id).unwrap();
    manager.cancel_action(&operation_id).unwrap();
    let result = first.await.unwrap();
    assert_eq!(
        result.as_ref().unwrap_err().code,
        WorkspaceCapabilityErrorCode::PrepareFailed
    );
    assert_eq!(duplicate.await, result);
    assert!(
        release.is_closed(),
        "Provider future must be dropped before completion"
    );
    assert_eq!(provider.prepares.load(Ordering::SeqCst), 1);
    assert_eq!(
        manager.cancel_action(&operation_id).unwrap_err().code,
        WorkspaceCapabilityErrorCode::NotFound
    );
    drop(manager.begin_workspace_remove(&target).await.unwrap());
    let events = lock_unpoisoned(&sink.0);
    assert_eq!(events.len(), 2);
    assert_eq!((events[0].state, events[1].state), ("running", "failed"));
    assert_eq!((events[0].revision, events[1].revision), (1, 2));
    assert_eq!(events[1].message_code, "CAPABILITY_ACTION_FAILED");
    for event in events.iter() {
        assert_eq!(event.operation_id, operation_id);
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 8);
        for key in [
            "operationId",
            "workspaceId",
            "providerId",
            "actionId",
            "stageCode",
            "state",
            "revision",
            "messageCode",
        ] {
            assert!(value.get(key).is_some());
        }
    }
}

/// 取消 ensure-runtime 的 startup leader 后，reservation 与 Starting 都收敛，后续可重新 acquire/stop。
#[tokio::test]
async fn explicit_cancel_startup_releases_pending_and_allows_reacquire() {
    let provider = action_provider(CapabilityActionExecution::ManagerEnsureRuntime, true);
    let release = provider.enqueue_start();
    let manager = runtime_manager(provider.clone());
    let sink = Arc::new(RecordingActionSink::default());
    let target = lease("cancel-start", 1);
    let operation = {
        let manager = manager.clone();
        let sink = sink.clone();
        let target = target.clone();
        tokio::spawn(async move {
            manager
                .prepare_action(target, "third-action", "prepare_index", sink)
                .await
        })
    };
    provider.start_entered.notified().await;
    let operation_id = lock_unpoisoned(&sink.0)[0].operation_id.clone();
    manager.cancel_action(&operation_id).unwrap();
    assert_eq!(
        operation.await.unwrap().unwrap_err().code,
        WorkspaceCapabilityErrorCode::PrepareFailed
    );
    assert!(release.is_closed());
    let slot = runtime_slot(&manager, "third-action", &target);
    {
        let state = lock_unpoisoned(&slot.state);
        assert_ne!(state.lifecycle, CapabilityRuntimeState::Starting);
        assert_eq!(state.pending_acquires, 0);
        assert_eq!(state.in_flight, 0);
    }
    drop(
        manager
            .acquire_runtime("third-action", target.clone())
            .await
            .unwrap(),
    );
    drop(manager.begin_workspace_remove(&target).await.unwrap());
    assert_eq!(provider.starts.load(Ordering::SeqCst), 2);
    assert_eq!(provider.stops.load(Ordering::SeqCst), 1);
}
