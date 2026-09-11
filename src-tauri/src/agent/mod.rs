//! TASK-001 foundation. Business creation, transitions and claim lifecycle follow later.
pub mod activity;
pub mod codex;
pub mod coordinator;
pub mod execution;
pub mod store;
#[cfg(windows)]
pub mod task_manager;

#[cfg(windows)]
pub mod product;
