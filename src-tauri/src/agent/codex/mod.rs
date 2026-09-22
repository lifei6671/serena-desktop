pub mod app_server;
#[cfg(windows)]
pub(crate) mod pool;
#[cfg(not(windows))]
#[path = "unavailable/pool.rs"]
pub(crate) mod pool;
pub mod protocol;
#[cfg(windows)]
pub mod provider;
#[cfg(not(windows))]
#[path = "unavailable/provider.rs"]
pub mod provider;
#[cfg(windows)]
pub mod runtime;
#[cfg(windows)]
pub mod windows_launcher;

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

#[cfg(windows)]
pub mod discovery;
#[cfg(not(windows))]
#[path = "unavailable/discovery.rs"]
pub mod discovery;
