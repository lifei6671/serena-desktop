pub mod app_server;
pub(crate) mod compatibility;
#[cfg(any(windows, target_os = "macos"))]
pub(crate) mod pool;
#[cfg(not(any(windows, target_os = "macos")))]
#[path = "unavailable/pool.rs"]
pub(crate) mod pool;
pub mod protocol;
#[cfg(any(windows, target_os = "macos"))]
pub mod provider;
#[cfg(not(any(windows, target_os = "macos")))]
#[path = "unavailable/provider.rs"]
pub mod provider;
#[cfg(windows)]
pub mod runtime;
#[cfg(windows)]
pub mod windows_launcher;

#[cfg(windows)]
pub(crate) mod platform_windows;
#[cfg(windows)]
pub(crate) use platform_windows as runtime_adapter;

#[cfg(target_os = "macos")]
// Phase 2A 冻结私有进程契约，Phase 2B 才接入产品路径。
#[allow(dead_code)]
pub(crate) mod macos_launcher;

#[cfg(target_os = "macos")]
// Phase 2A 只冻结当前 Host 连续 ownership 的 Runtime 收口；Phase 2B 才处理恢复与 Claim。
#[allow(dead_code)]
pub(crate) mod macos_runtime;

#[cfg(target_os = "macos")]
pub(crate) mod macos_runtime_store;

#[cfg(target_os = "macos")]
pub(crate) mod macos_recovery;

#[cfg(target_os = "macos")]
pub(crate) mod macos_discovery;

#[cfg(target_os = "macos")]
pub(crate) mod macos_runtime_adapter;
#[cfg(target_os = "macos")]
pub(crate) use macos_runtime_adapter as runtime_adapter;

#[cfg(windows)]
pub mod discovery;
#[cfg(not(any(windows, target_os = "macos")))]
#[path = "unavailable/discovery.rs"]
pub mod discovery;

#[cfg(test)]
tokio::task_local! {
    /// 只在测试作用域替代真实 CLI discovery，锁定首次 execute 的惰性探测路径。
    pub(crate) static TEST_BACKEND_DISCOVERY: Result<std::path::PathBuf, String>;
}

/// 平台私有 discovery 边界；macOS 消费正式 probe authority，其他平台保持原发现行为。
pub(crate) async fn discover(
    context: crate::agent::task_manager::ProbeContext,
) -> Result<std::path::PathBuf, String> {
    #[cfg(test)]
    if let Ok(result) = TEST_BACKEND_DISCOVERY.try_with(Clone::clone) {
        drop(context);
        return result;
    }
    #[cfg(target_os = "macos")]
    {
        macos_discovery::discover(context).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        drop(context);
        discovery::discover().await
    }
}

/// 共享 Provider 的 managed connect 边界；Windows 调用原函数，macOS 额外传递正式 probe authority。
#[cfg(any(windows, target_os = "macos"))]
pub(crate) async fn connect_managed(
    store: crate::agent::store::StateStore,
    owner: String,
    runtime_pool: std::sync::Arc<pool::CodexRuntimePool>,
    runtime_id: String,
    executable: std::path::PathBuf,
    cwd: std::path::PathBuf,
    attempt: Option<app_server::managed::RuntimeAttempt>,
) -> Result<app_server::managed::ManagedClient, runtime_adapter::RuntimeFailure> {
    #[cfg(target_os = "macos")]
    {
        let context = crate::agent::task_manager::ProbeContext::from_existing(
            store.clone(),
            owner.clone(),
            runtime_pool,
        );
        app_server::managed::connect(context, store, owner, runtime_id, executable, cwd, attempt)
            .await
    }
    #[cfg(windows)]
    {
        drop(runtime_pool);
        app_server::managed::connect(store, owner, runtime_id, executable, cwd, attempt).await
    }
}

#[cfg(all(test, target_os = "macos"))]
mod platform_selection_tests {
    /// macOS 必须编译唯一共享 Provider，而不是 unavailable 或平台副本。
    #[test]
    fn macos_selects_shared_provider_and_pool() {
        fn assert_provider<T: crate::agent::provider::port::AgentProvider>() {}
        assert_provider::<super::provider::CodexProvider>();
        let pool = super::pool::CodexRuntimePool::default();
        assert!(!pool.stop.is_cancelled());
    }
}
