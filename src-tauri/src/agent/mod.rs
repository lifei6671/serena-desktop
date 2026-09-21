//! TASK-001 foundation. Business creation, transitions and claim lifecycle follow later.
pub mod activity;
pub mod codex;
pub mod coordinator;
pub mod execution;
pub(crate) mod notification;
pub mod provider;
pub mod store;
#[cfg(windows)]
pub mod task_manager;
pub mod telemetry_projector;
pub mod usage;

#[cfg(windows)]
pub mod product;

#[cfg(windows)]
pub mod work;
