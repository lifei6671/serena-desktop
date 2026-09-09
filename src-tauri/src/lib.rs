#[allow(
    dead_code,
    reason = "TASK-001 foundation is retained without production consumers until Agent lifecycle integration"
)]
mod agent;
mod commands;
mod config;
mod discovery;
mod installer;
mod logs;
mod mcp;
mod serena;
mod tray;

use config::AppPaths;
use serena::SupervisorState;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

struct ShutdownState {
    started: AtomicBool,
    ready: AtomicBool,
}

impl Default for ShutdownState {
    fn default() -> Self {
        Self {
            started: AtomicBool::new(false),
            ready: AtomicBool::new(false),
        }
    }
}

fn is_autostart_launch(arguments: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>) -> bool {
    arguments
        .into_iter()
        .any(|argument| argument.as_ref() == "--autostart")
}

pub(crate) fn request_exit(app: &AppHandle) {
    let shutdown = app.state::<ShutdownState>();
    if shutdown.started.swap(true, Ordering::AcqRel) {
        return;
    }

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || match commands::shutdown_impl(&handle) {
        Ok(()) => {
            handle
                .state::<ShutdownState>()
                .ready
                .store(true, Ordering::Release);
            handle.exit(0);
        }
        Err(error) => {
            logs::append(
                &handle
                    .state::<std::sync::Arc<SupervisorState>>()
                    .paths
                    .app_log,
                "shutdown",
                &error,
            );
            handle
                .state::<ShutdownState>()
                .started
                .store(false, Ordering::Release);
            tray::show_main_window(&handle);
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, arguments, _cwd| {
                if !is_autostart_launch(&arguments) {
                    tray::show_main_window(app);
                }
            },
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .setup(|app| {
            #[cfg(windows)]
            app.get_webview_window("main")
                .expect("main webview window must exist")
                .with_webview(|webview| unsafe {
                    // Tauri runs this callback on the WebView2 UI thread.
                    webview
                        .controller()
                        .CoreWebView2()
                        .and_then(|core| core.Settings())
                        .and_then(|settings| settings.SetAreDefaultContextMenusEnabled(false))
                        .expect("failed to disable default WebView2 context menus");
                })?;

            let paths = AppPaths::resolve(app.handle()).map_err(std::io::Error::other)?;
            let supervisor =
                std::sync::Arc::new(SupervisorState::new(paths).map_err(std::io::Error::other)?);
            app.manage(supervisor.clone());
            let broker = std::sync::Arc::new(mcp::Broker::new(supervisor));
            let store = tauri::async_runtime::block_on(agent::store::StateStore::open_for_app(
                app.handle(),
            ))
            .map_err(std::io::Error::other)?;
            let (product, outcomes) = tauri::async_runtime::block_on(
                agent::product::AgentProductService::initialize(store),
            ).map_err(std::io::Error::other)?;
            if let Some(error) = product.backend_diagnostic() {
                logs::append(&app.state::<std::sync::Arc<SupervisorState>>().paths.app_log, "agent backend", &format!("Agent backend unavailable: {error}"));
            }
            for outcome in outcomes {
                use agent::task_manager::recovery::RecoveryOutcome::*;
                let (kind, id) = match &outcome {
                    Released { execution_id } => ("released", execution_id),
                    Inconsistent { execution_id, .. } => ("inconsistent; claim retained", execution_id),
                    PendingExplicitResume { execution_id } => ("pending explicit resume", execution_id),
                    Unknown { execution_id, .. } => ("unknown; claim retained", execution_id),
                    RuntimeFailure { execution_id, .. } => ("runtime failure; claim retained", execution_id),
                    Interrupted { execution, .. } => ("interrupted; safely released", &execution.id),
                };
                logs::append(&app.state::<std::sync::Arc<SupervisorState>>().paths.app_log, "agent recovery", &format!("{id}: {kind}"));
            }
            let product = std::sync::Arc::new(product);
            let _ = broker.product.set(product.clone());
            app.manage(product);
            app.manage(broker.clone());
            let sync_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = commands::sync_workspaces(sync_app).await {
                    *broker.sync_warnings.lock().unwrap() = vec![format!("同步失败：{e}")];
                }
                if broker.config().broker.enabled
                    && let Err(e) = broker.start().await
                {
                    *broker.error.lock().unwrap() = Some(e);
                }
            });
            app.manage(ShutdownState::default());
            tray::create(app.handle())?;

            if is_autostart_launch(std::env::args_os()) {
                if let Some(window) = app.get_webview_window("main") {
                    window.hide()?;
                }
            } else {
                tray::show_main_window(app.handle());
            }

            let handle = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                let installation = handle
                    .state::<std::sync::Arc<SupervisorState>>()
                    .detect_serena();
                if installation
                    .is_some_and(|value| value.state == discovery::InstallationState::Standard)
                {
                    let supervisor = handle.state::<std::sync::Arc<SupervisorState>>();
                    let broker = mcp::get(&handle);
                    let result = tauri::async_runtime::block_on(async {
                        let _m = broker.management.lock().await;
                        let _slot = broker.workspace.write().await;
                        supervisor.start_automatically_if(|| {
                            !handle
                                .state::<ShutdownState>()
                                .started
                                .load(Ordering::Acquire)
                        })
                    });
                    if let Err(error) = result {
                        logs::append(&supervisor.paths.app_log, "startup", &error);
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let app = window.app_handle();
                let minimize = app
                    .state::<std::sync::Arc<SupervisorState>>()
                    .snapshot()
                    .config
                    .minimize_to_tray;
                if minimize {
                    let _ = window.hide();
                } else {
                    request_exit(app);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::agent_operation,
            commands::get_app_state,
            commands::get_codex_version,
            commands::get_broker_state,
            commands::get_mcp_logs,
            commands::clear_mcp_logs,
            commands::download_mcp_logs,
            commands::sync_workspaces,
            commands::activate_workspace,
            commands::deactivate_workspace,
            commands::cancel_workspace_operation,
            commands::set_broker,
            commands::detect_serena,
            commands::detect_git,
            commands::repair_serena,
            commands::install_serena,
            commands::start_serena,
            commands::stop_serena,
            commands::restart_serena,
            commands::save_config,
            commands::set_autostart,
            commands::open_dashboard,
            commands::open_log_directory,
            commands::open_external_url,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Serena Desktop");

    app.run(|app, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            let shutdown = app.state::<ShutdownState>();
            if !shutdown.ready.load(Ordering::Acquire) {
                api.prevent_exit();
                request_exit(app);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::is_autostart_launch;

    #[test]
    fn recognizes_only_explicit_autostart_argument() {
        assert!(is_autostart_launch(["serena-desktop.exe", "--autostart"]));
        assert!(!is_autostart_launch(["serena-desktop.exe"]));
        assert!(!is_autostart_launch([
            "serena-desktop.exe",
            "--autostart=true"
        ]));
    }
}
