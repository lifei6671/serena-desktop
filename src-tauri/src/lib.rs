#[allow(
    dead_code,
    reason = "TASK-001 foundation is retained without production consumers until Agent lifecycle integration"
)]
mod agent;
mod agent_notification;
#[cfg(windows)]
mod autostart;
mod codegraph_capability;
mod commands;
mod config;
mod discovery;
mod installer;
#[cfg(windows)]
mod load_error;
mod logs;
#[cfg(target_os = "macos")]
mod macos_process;
mod mcp;
mod oauth;
mod remote;
mod serena;
mod serena_capability;
mod tray;
mod workspace_capability;
mod workspace_inspection;
mod workspace_path;
mod workspace_picker;
mod workspace_registry;
mod workspace_resolver;

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

/// 对所有可感知退出提供单一、可重试且成功后幂等的 shutdown gate。
fn run_shutdown_once(
    state: &ShutdownState,
    shutdown: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if state.ready.load(Ordering::Acquire) {
        return Ok(());
    }
    if state.started.swap(true, Ordering::AcqRel) {
        return Err("退出清理正在进行".into());
    }
    match shutdown() {
        Ok(()) => {
            state.ready.store(true, Ordering::Release);
            Ok(())
        }
        Err(error) => {
            state.started.store(false, Ordering::Release);
            Err(error)
        }
    }
}

fn is_autostart_launch(arguments: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>) -> bool {
    arguments
        .into_iter()
        .any(|argument| argument.as_ref() == "--autostart")
}

/// macOS Dock reopen 在没有可见窗口时需要恢复主窗口。
#[cfg(target_os = "macos")]
fn should_show_main_on_reopen(has_visible_windows: bool) -> bool {
    !has_visible_windows
}

pub(crate) async fn finish_broker_startup(
    broker: std::sync::Arc<mcp::Broker>,
    serena_startup: tauri::async_runtime::JoinHandle<()>,
) -> Result<(), String> {
    broker.startup_after_serena(serena_startup).await
}

pub(crate) fn request_exit(app: &AppHandle) {
    let shutdown = app.state::<ShutdownState>();
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    match run_shutdown_once(&shutdown, || commands::shutdown_impl(app)) {
        Ok(()) => {
            app.exit(0);
        }
        Err(error) => {
            logs::append(
                &app.state::<std::sync::Arc<SupervisorState>>().paths.app_log,
                "shutdown",
                &error,
            );
            tray::show_main_window(app);
        }
    }
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
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .setup(|app| {
            #[cfg(windows)]
            load_error::install(&app.get_webview_window("main").expect("main window"))?;
            #[cfg(windows)]
            autostart::refresh_enabled_registration(app.handle()).map_err(std::io::Error::other)?;
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
            let broker = std::sync::Arc::new(mcp::Broker::new(supervisor.clone()));
            let _ = broker.remote.app.set(app.handle().clone());
            let store = tauri::async_runtime::block_on(agent::store::StateStore::open_for_app(
                app.handle(),
            ))
            .map_err(std::io::Error::other)?;
            let terminal_notifier =
                std::sync::Arc::new(agent_notification::DesktopAgentTerminalNotifier::new(
                    app.handle().clone(),
                    supervisor,
                ));
            let (product, outcomes) = tauri::async_runtime::block_on(
                agent::product::AgentProductService::initialize_with_terminal_notifier(
                    store,
                    terminal_notifier,
                ),
            )
            .map_err(std::io::Error::other)?;
            if let Some(error) = product.backend_diagnostic() {
                logs::append(
                    &app.state::<std::sync::Arc<SupervisorState>>().paths.app_log,
                    "agent backend",
                    &format!("Agent backend unavailable: {error}"),
                );
            }
            for item in outcomes {
                use agent::provider::port::ProviderReconcileKind::*;
                let kind = match item.kind {
                    OrphanResourceRecovered => "orphan resource terminated",
                    OrphanResourceUnknown => "orphan resource unknown",
                    ExecutionReleased => "released",
                    ExecutionInconsistent => "inconsistent; claim retained",
                    ExecutionPendingExplicitResume => "pending explicit resume",
                    ExecutionUnknown => "unknown; claim retained",
                    ExecutionProviderFailure => "provider failure; claim retained",
                    ExecutionInterrupted => "interrupted; safely released",
                };
                logs::append(
                    &app.state::<std::sync::Arc<SupervisorState>>().paths.app_log,
                    "agent recovery",
                    &format!("{}: {kind}", item.subject_id),
                );
            }
            let product = std::sync::Arc::new(product);
            let _ = broker.product.set(product.clone());
            app.manage(product);
            app.manage(broker.clone());
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
            let serena_startup = tauri::async_runtime::spawn_blocking(move || {
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
            tauri::async_runtime::spawn(async move {
                if let Err(e) = finish_broker_startup(broker.clone(), serena_startup).await {
                    *broker.error.lock().unwrap() = Some(e);
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
            remote::remote_state,
            remote::remote_save_ngrok_auth,
            remote::remote_clear_ngrok_auth,
            remote::remote_start_ngrok,
            remote::remote_start,
            remote::remote_stop,
            remote::remote_probe,
            remote::remote_approve,
            commands::agent_operation,
            commands::agent_manual_resolve,
            commands::agent_history,
            commands::get_app_state,
            commands::workspace_list,
            commands::workspace_get,
            commands::workspace_select,
            commands::workspace_register,
            commands::workspace_rename,
            commands::workspace_reorder,
            commands::workspace_remove,
            commands::workspace_capability_observe,
            commands::workspace_capability_prepare,
            commands::workspace_capability_cancel,
            workspace_inspection::workspace_inspect_directory,
            workspace_picker::workspace_pick_directory,
            commands::get_codex_version,
            commands::get_broker_state,
            commands::get_mcp_logs,
            commands::clear_mcp_logs,
            commands::download_mcp_logs,
            commands::workspace_import_serena,
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

    app.run(|app, event| match event {
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows,
            ..
        } => {
            if should_show_main_on_reopen(has_visible_windows) {
                tray::show_main_window(app);
            }
        }
        RunEvent::ExitRequested { api, .. } => {
            let shutdown = app.state::<ShutdownState>();
            if !shutdown.ready.load(Ordering::Acquire) {
                api.prevent_exit();
                request_exit(app);
            }
        }
        #[cfg(target_os = "macos")]
        RunEvent::Exit => {
            let shutdown = app.state::<ShutdownState>();
            if let Err(error) = run_shutdown_once(&shutdown, || commands::shutdown_impl(app)) {
                logs::append(
                    &app.state::<std::sync::Arc<SupervisorState>>().paths.app_log,
                    "shutdown",
                    &error,
                );
            }
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::{ShutdownState, is_autostart_launch, run_shutdown_once};
    use std::{cell::Cell, sync::atomic::Ordering};

    /// shutdown 成功后最终 Exit 不得再次调用 owner。
    #[test]
    fn shutdown_once_marks_ready_and_skips_reentry() {
        let state = ShutdownState::default();
        let calls = Cell::new(0);
        run_shutdown_once(&state, || {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .unwrap();
        run_shutdown_once(&state, || {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert!(state.ready.load(Ordering::Acquire));
    }

    /// 可取消退出失败后必须允许用户再次触发完整 shutdown。
    #[test]
    fn shutdown_once_failure_allows_retry() {
        let state = ShutdownState::default();
        assert_eq!(
            run_shutdown_once(&state, || Err("fixture failure".into())),
            Err("fixture failure".into())
        );
        assert!(!state.started.load(Ordering::Acquire));
        run_shutdown_once(&state, || Ok(())).unwrap();
        assert!(state.ready.load(Ordering::Acquire));
    }

    #[test]
    fn recognizes_only_explicit_autostart_argument() {
        assert!(is_autostart_launch(["serena-desktop.exe", "--autostart"]));
        assert!(!is_autostart_launch(["serena-desktop.exe"]));
        assert!(!is_autostart_launch([
            "serena-desktop.exe",
            "--autostart=true"
        ]));
    }

    /// Dock reopen 只在应用没有可见窗口时恢复主窗口。
    #[cfg(target_os = "macos")]
    #[test]
    fn dock_reopen_only_restores_when_no_window_is_visible() {
        assert!(super::should_show_main_on_reopen(false));
        assert!(!super::should_show_main_on_reopen(true));
    }
}
