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

#[cfg(windows)]
pub mod discovery;
#[cfg(not(windows))]
#[path = "unavailable/discovery.rs"]
pub mod discovery;
