use crate::{commands, logs, serena::SupervisorState};
use tauri::{
    AppHandle, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "Serena MCP", false, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "打开主界面", true, None::<&str>)?;
    let dashboard = MenuItem::with_id(app, "dashboard", "打开 Dashboard", true, None::<&str>)?;
    let logs_item = MenuItem::with_id(app, "logs", "打开日志目录", true, None::<&str>)?;
    let start = MenuItem::with_id(app, "start", "启动 Serena", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "停止 Serena", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", "重新启动 Serena", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let separator_one = PredefinedMenuItem::separator(app)?;
    let separator_two = PredefinedMenuItem::separator(app)?;
    let separator_three = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &separator_one,
            &show,
            &dashboard,
            &logs_item,
            &separator_two,
            &start,
            &stop,
            &restart,
            &separator_three,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id("main-tray")
        .icon(
            app.default_window_icon()
                .expect("application icon must be configured")
                .clone(),
        )
        .tooltip("Serena Desktop")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "dashboard" => {
                if let Err(error) = commands::open_dashboard(app.clone()) {
                    log_tray_error(app, &error);
                }
            }
            "logs" => {
                if let Err(error) = commands::open_log_directory(app.clone()) {
                    log_tray_error(app, &error);
                }
            }
            "start" => {
                let handle = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Err(error) = commands::start_impl(&handle) {
                        log_tray_error(&handle, &error);
                    }
                });
            }
            "stop" => {
                let handle = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Err(error) = commands::stop_impl(&handle) {
                        log_tray_error(&handle, &error);
                    }
                });
            }
            "restart" => {
                let handle = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let result = commands::restart_impl(&handle);
                    if let Err(error) = result {
                        log_tray_error(&handle, &error);
                    }
                });
            }
            "quit" => crate::request_exit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn log_tray_error(app: &AppHandle, error: &str) {
    let path = app
        .state::<std::sync::Arc<SupervisorState>>()
        .paths
        .app_log
        .clone();
    logs::append(&path, "tray", error);
}
