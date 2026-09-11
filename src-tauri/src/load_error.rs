//! Windows main-frame navigation fallback, independent of the frontend and IPC.
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{cell::{Cell, RefCell}, rc::Rc};
use tauri::{Manager, WebviewWindow};
use webview2_com::{
    CoTaskMemPWSTR,
    Microsoft::Web::WebView2::Win32::COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED,
    NavigationCompletedEventHandler, NavigationStartingEventHandler,
};
use windows::core::{HSTRING, PWSTR};

fn page(code: i32) -> String {
    include_str!("load_error.html")
        .replace(
            "__ICON__",
            &STANDARD.encode(include_bytes!("../icons/32x32.png")),
        )
        // Never reflect the failed URL: its query/path may contain credentials.
        .replace(
            "__DETAIL__",
            &format!("主界面导航失败\nWebView2 错误码：{code}"),
        )
}

fn action(active: bool, uri: &str) -> Option<&str> {
    if !active {
        return None;
    }
    match uri {
        "https://serena-recovery.invalid/reload" => Some("reload"),
        "https://serena-recovery.invalid/logs" => Some("logs"),
        "https://serena-recovery.invalid/exit" => Some("exit"),
        _ => None,
    }
}

pub fn install(window: &WebviewWindow) -> tauri::Result<()> {
    let initial = window.url()?;
    let app = window.app_handle().clone();
    window.with_webview(move |webview| {
        // All COM calls and the Rc<Cell> remain on WebView2's UI thread.
        let install = || -> windows::core::Result<()> { unsafe {
            let core = webview.controller().CoreWebView2()?;
            let mut target = (initial.as_str() != "about:blank").then(|| initial.to_string());
            let fallback_uri = Rc::new(RefCell::new(String::new()));
            let allowed_uri = fallback_uri.clone();
            let active = Rc::new(Cell::new(false));
            let shown = active.clone();
            let mut token = 0;
            core.add_NavigationStarting(&NavigationStartingEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()); };
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = CoTaskMemPWSTR::from(uri).to_string();
                if !active.get() && target.is_none() && uri != "about:blank" {
                    target = Some(uri.clone());
                }
                if let Some(command) = action(active.get(), &uri) {
                    args.SetCancel(true)?;
                    match command {
                        "reload" => {
                            active.set(false);
                            if let Some(target) = &target { sender.Navigate(&HSTRING::from(target.as_str()))?; }
                        }
                        "logs" => {
                            if let Err(error) = crate::commands::open_log_directory(app.clone()) {
                                crate::logs::append(&app.state::<std::sync::Arc<crate::serena::SupervisorState>>().paths.app_log, "load error", &error);
                            }
                        }
                        "exit" => crate::request_exit(&app),
                        _ => unreachable!(),
                    }
                } else if active.get() && uri != *allowed_uri.borrow() {
                    // The static fallback has no other navigation destinations.
                    args.SetCancel(true)?;
                }
                Ok(())
            })), &mut token)?;
            core.add_NavigationCompleted(&NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()); };
                let mut success = Default::default();
                args.IsSuccess(&mut success)?;
                if !success.as_bool() && !shown.get() {
                    let mut status = Default::default();
                    args.WebErrorStatus(&mut status)?;
                    if status != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED {
                        shown.set(true);
                        let uri = format!("data:text/html;charset=utf-8;base64,{}", STANDARD.encode(page(status.0)));
                        *fallback_uri.borrow_mut() = uri.clone();
                        if let Err(error) = sender.Navigate(&HSTRING::from(uri)) {
                            shown.set(false);
                            return Err(error);
                        }
                    }
                }
                Ok(())
            })), &mut token)?;
            core.Settings()?.SetIsBuiltInErrorPageEnabled(false)?;
            Ok(())
        }};
        install().expect("failed to install main-window navigation fallback");
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_commands_are_exact_and_only_available_on_fallback() {
        for name in ["reload", "logs", "exit"] {
            let uri = format!("https://serena-recovery.invalid/{name}");
            assert_eq!(action(true, &uri), Some(name));
            assert_eq!(action(false, &uri), None);
            assert_eq!(action(true, &format!("{uri}?token=secret")), None);
        }
        assert_eq!(action(true, "https://example.com/exit"), None);
    }
    #[test]
    fn fallback_is_self_contained_and_has_no_frontend_or_ipc_dependency() {
        let html = page(12);
        assert!(html.contains("WebView2 错误码：12"));
        assert!(html.contains("data:image/png;base64,"));
        assert!(!html.contains("__ICON__"));
        assert!(!html.contains("<script"));
        assert!(!html.contains("localhost"));
    }

    #[test]
    #[ignore = "native WebView2 smoke; creates an isolated hidden window and loopback server"]
    fn native_failed_navigation_and_retry_recover_without_frontend() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::{Arc, Mutex},
            time::{Duration, Instant},
        };
        use webview2_com::ExecuteScriptCompletedHandler;
        let directory = tempfile::tempdir().unwrap();
        let data_directory = directory.path().to_path_buf();
        let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reserved.local_addr().unwrap();
        drop(reserved);
        let observed = Arc::new(Mutex::new(Vec::new()));
        let results = observed.clone();
        let mut context = tauri::generate_context!();
        context.config_mut().app.windows.clear();
        let app = tauri::Builder::default().any_thread().setup(move |app| {
            let window = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(format!("http://{address}/").parse().unwrap()))
                .data_directory(data_directory).visible(false).build()?;
            install(&window)?;
            let handle = app.handle().clone();
            window.with_webview(move |webview| unsafe {
                let core = webview.controller().CoreWebView2().unwrap();
                let mut token = 0;
                let first = Cell::new(true);
                core.add_NavigationCompleted(&NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
                    let (Some(sender), Some(args)) = (sender, args) else { return Ok(()); };
                    let mut success = Default::default();
                    args.IsSuccess(&mut success)?;
                    if !success.as_bool() { return Ok(()); }
                    let initial = first.replace(false);
                    let script = if initial {
                        "document.title.includes('界面加载失败') && document.querySelectorAll('nav a').length === 3 && !document.querySelector('details').open"
                    } else { "document.body.textContent.includes('Recovered')" };
                    let results = results.clone();
                    let handle = handle.clone();
                    let retry = sender.clone();
                    sender.ExecuteScript(&HSTRING::from(script), &ExecuteScriptCompletedHandler::create(Box::new(move |status, value| {
                        status?;
                        results.lock().unwrap().push(value == "true");
                        if initial {
                            let listener = TcpListener::bind(address).unwrap();
                            listener.set_nonblocking(true).unwrap();
                            std::thread::spawn(move || {
                                let deadline = Instant::now() + Duration::from_secs(10);
                                while Instant::now() < deadline {
                                    if let Ok((mut stream, _)) = listener.accept() {
                                        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                                        let mut buf = [0; 4096];
                                        let _ = stream.read(&mut buf);
                                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 35\r\nConnection: close\r\n\r\n<html><body>Recovered</body></html>");
                                        break;
                                    }
                                    std::thread::sleep(Duration::from_millis(20));
                                }
                            });
                            retry.ExecuteScript(&HSTRING::from("document.querySelector('nav a').click()"), &ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(()))))?;
                        } else { handle.exit(0); }
                        Ok(())
                    })))?;
                    Ok(())
                })), &mut token).unwrap();
            })?;
            Ok(())
        }).build(context).unwrap();
        let timeout = app.handle().clone();
        let (done, wait) = std::sync::mpsc::channel();
        let watchdog = std::thread::spawn(move || {
            if wait.recv_timeout(Duration::from_secs(20)).is_err() {
                timeout.exit(1);
            }
        });
        let code = app.run_return(|_, _| {});
        let _ = done.send(());
        watchdog.join().unwrap();
        assert_eq!(code, 0);
        assert_eq!(*observed.lock().unwrap(), vec![true, true]);
    }
}
