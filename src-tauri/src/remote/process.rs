//! Quick Tunnel ownership only; no Runtime store or recovery state machine.
#[cfg(windows)]
#[path = "process_windows.rs"]
mod windows;
#[cfg(windows)]
pub(super) use windows::ManagedChild;

#[cfg(target_os = "macos")]
#[path = "process_macos.rs"]
mod macos;
#[cfg(target_os = "macos")]
pub(super) use macos::ManagedChild;

#[cfg(all(not(windows), not(target_os = "macos")))]
pub(super) struct ManagedChild(tokio::process::Child);
#[cfg(all(not(windows), not(target_os = "macos")))]
impl ManagedChild {
    pub(super) fn spawn(command: &mut tokio::process::Command) -> Result<Self, String> {
        command
            .spawn()
            .map(Self)
            .map_err(|_| "QUICK_TUNNEL_START_FAILED".into())
    }
}
#[cfg(all(not(windows), not(target_os = "macos")))]
impl std::ops::Deref for ManagedChild {
    type Target = tokio::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
#[cfg(all(not(windows), not(target_os = "macos")))]
impl std::ops::DerefMut for ManagedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
