mod commands;
mod config;
mod installer;
mod logs;
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
    tauri::async_runtime::spawn_blocking(move || match handle.state::<SupervisorState>().stop() {
        Ok(()) => {
            handle
                .state::<ShutdownState>()
                .ready
                .store(true, Ordering::Release);
            handle.exit(0);
        }
        Err(error) => {
            logs::append(
                &handle.state::<SupervisorState>().paths.app_log,
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
            let supervisor = SupervisorState::new(paths).map_err(std::io::Error::other)?;
            app.manage(supervisor);
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
                let installation = handle.state::<SupervisorState>().detect_serena();
                if installation.is_some() {
                    let supervisor = handle.state::<SupervisorState>();
                    let result = supervisor.start_automatically_if(|| {
                        !handle
                            .state::<ShutdownState>()
                            .started
                            .load(Ordering::Acquire)
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
                    .state::<SupervisorState>()
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
            commands::get_app_state,
            commands::detect_serena,
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
