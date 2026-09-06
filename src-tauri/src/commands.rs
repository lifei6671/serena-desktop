use crate::{config::ManagerConfig, installer, serena::SupervisorState};
use serde::Serialize;
use std::{path::Path, process::Stdio};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    config: ManagerConfig,
    installation: Option<crate::serena::SerenaInstallation>,
    active_installation: Option<crate::serena::SerenaInstallation>,
    server_status: crate::serena::ServerStatus,
    managed_process_present: bool,
    active_port: u16,
    endpoint: String,
    dashboard_url: String,
    dashboard_enabled: bool,
    log_directory: String,
    autostart_enabled: Option<bool>,
    autostart_error: Option<String>,
    last_error: Option<String>,
}

pub fn build_app_state(app: &AppHandle, known_autostart: Option<bool>) -> AppState {
    let supervisor = app.state::<SupervisorState>();
    let snapshot = supervisor.snapshot();
    let (autostart_enabled, autostart_error) = resolve_autostart(known_autostart, || {
        app.autolaunch()
            .is_enabled()
            .map_err(|error| error.to_string())
    });
    AppState {
        endpoint: format!("http://127.0.0.1:{}/mcp", snapshot.active_port),
        log_directory: supervisor.paths.log_directory.display().to_string(),
        config: snapshot.config,
        installation: snapshot.installation,
        active_installation: snapshot.active_installation,
        server_status: snapshot.server_status,
        managed_process_present: snapshot.managed_process_present,
        active_port: snapshot.active_port,
        dashboard_url: snapshot.dashboard_url,
        dashboard_enabled: snapshot.active_dashboard_enabled,
        autostart_enabled,
        autostart_error,
        last_error: snapshot.last_error,
    }
}

fn resolve_autostart(
    known: Option<bool>,
    read: impl FnOnce() -> Result<bool, String>,
) -> (Option<bool>, Option<String>) {
    match known {
        Some(enabled) => (Some(enabled), None),
        None => match read() {
            Ok(enabled) => (Some(enabled), None),
            Err(error) => (
                None,
                Some(format!("无法读取 Windows 登录自启状态：{error}")),
            ),
        },
    }
}

#[tauri::command]
pub fn get_app_state(app: AppHandle) -> Result<AppState, String> {
    Ok(build_app_state(&app, None))
}

#[tauri::command]
pub fn detect_serena(app: AppHandle) -> Result<AppState, String> {
    app.state::<SupervisorState>().detect_serena();
    Ok(build_app_state(&app, None))
}

#[tauri::command]
pub async fn install_serena(app: AppHandle) -> Result<AppState, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let supervisor = handle.state::<SupervisorState>();
        if supervisor.snapshot().installation.is_none() {
            installer::install_serena(&supervisor.paths.app_log)?;
            if supervisor.detect_serena().is_none() {
                return Err("安装命令已完成，但重新检测后仍未发现 Serena。".into());
            }
        }
        Ok(build_app_state(&handle, None))
    })
    .await
    .map_err(|error| format!("安装任务异常结束：{error}"))?
}

pub fn start_impl(app: &AppHandle) -> Result<AppState, String> {
    app.state::<SupervisorState>().start()?;
    Ok(build_app_state(app, None))
}

#[tauri::command]
pub async fn start_serena(app: AppHandle) -> Result<AppState, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || start_impl(&handle))
        .await
        .map_err(|error| format!("启动任务异常结束：{error}"))?
}

pub fn stop_impl(app: &AppHandle) -> Result<AppState, String> {
    app.state::<SupervisorState>().stop()?;
    Ok(build_app_state(app, None))
}

#[tauri::command]
pub async fn stop_serena(app: AppHandle) -> Result<AppState, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || stop_impl(&handle))
        .await
        .map_err(|error| format!("停止任务异常结束：{error}"))?
}

#[tauri::command]
pub async fn restart_serena(app: AppHandle) -> Result<AppState, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        handle.state::<SupervisorState>().restart()?;
        Ok(build_app_state(&handle, None))
    })
    .await
    .map_err(|error| format!("重启任务异常结束：{error}"))?
}

#[tauri::command]
pub fn save_config(app: AppHandle, config: ManagerConfig) -> Result<AppState, String> {
    app.state::<SupervisorState>().replace_config(config)?;
    Ok(build_app_state(&app, None))
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<AppState, String> {
    let manager = app.autolaunch();
    if enabled {
        manager
            .enable()
            .map_err(|error| format!("无法启用 Windows 登录自启：{error}"))?;
    } else {
        manager
            .disable()
            .map_err(|error| format!("无法关闭 Windows 登录自启：{error}"))?;
    }
    Ok(build_app_state(&app, Some(enabled)))
}

#[tauri::command]
pub fn open_dashboard(app: AppHandle) -> Result<(), String> {
    let snapshot = app.state::<SupervisorState>().snapshot();
    if !snapshot.active_dashboard_enabled {
        return Err("Dashboard 已关闭。".into());
    }
    if snapshot.server_status != crate::serena::ServerStatus::Running {
        return Err("Serena 尚未运行，无法打开 Dashboard。".into());
    }
    open_with_system(&snapshot.dashboard_url)
}

#[tauri::command]
pub fn open_log_directory(app: AppHandle) -> Result<(), String> {
    let path = app.state::<SupervisorState>().paths.log_directory.clone();
    open_with_system(&path)
}

#[tauri::command]
pub fn open_external_url(target: &str) -> Result<(), String> {
    let url = match target {
        "docs" => "https://oraios.github.io/serena/02-usage/010_installation.html",
        "github" => "https://github.com/oraios/serena",
        _ => return Err("不支持的外部链接。".into()),
    };
    open_with_system(url)
}

fn open_with_system(target: impl AsRef<Path>) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
        std::process::Command::new("explorer.exe")
            .arg(target.as_ref())
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("无法打开 {}：{error}", target.as_ref().display()))
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(target.as_ref())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("无法打开 {}：{error}", target.as_ref().display()))
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_autostart;

    #[test]
    fn autostart_read_failure_is_reported_without_failing_app_state() {
        let (enabled, error) = resolve_autostart(None, || Err("registry unavailable".into()));

        assert_eq!(enabled, None);
        assert_eq!(
            error.as_deref(),
            Some("无法读取 Windows 登录自启状态：registry unavailable")
        );
    }

    #[test]
    fn known_autostart_value_does_not_read_registry_again() {
        let (enabled, error) = resolve_autostart(Some(true), || {
            panic!("known value must bypass the registry read")
        });

        assert_eq!(enabled, Some(true));
        assert_eq!(error, None);
    }
}
