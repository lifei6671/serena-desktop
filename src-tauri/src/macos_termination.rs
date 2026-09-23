use std::future::Future;

use tauri::{AppHandle, Manager};
use tokio::signal::unix::{SignalKind, signal};

/// 安装 macOS SIGTERM 监听，并把退出请求投递给现有 Tauri 主线程 authority。
pub(crate) fn install(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut termination = match signal(SignalKind::terminate()) {
            Ok(termination) => termination,
            Err(error) => {
                log_listener_error(
                    &app,
                    &format!("macOS SIGTERM listener initialization failed: {error}"),
                );
                return;
            }
        };

        dispatch_termination_once(termination.recv(), || {
            dispatch_exit_on_main_thread(app);
        })
        .await;
    });
}

/// 只消费首个 SIGTERM 并投递一次退出 callback，随后由调用方结束监听任务。
async fn dispatch_termination_once(
    receive: impl Future<Output = Option<()>>,
    dispatch: impl FnOnce(),
) -> bool {
    if receive.await.is_some() {
        dispatch();
        true
    } else {
        false
    }
}

/// 将 SIGTERM 转换为主线程上的统一 request_exit，不在 signal task 中直接清理资源。
fn dispatch_exit_on_main_thread(app: AppHandle) {
    let request_app = app.clone();
    if let Err(error) = app.run_on_main_thread(move || crate::request_exit(&request_app)) {
        log_listener_error(
            &app,
            &format!("macOS SIGTERM dispatch to main thread failed: {error}"),
        );
    }
}

/// 将监听或投递失败写入稳定应用日志，但不阻断应用启动。
fn log_listener_error(app: &AppHandle, message: &str) {
    crate::logs::append(
        &app.state::<std::sync::Arc<crate::SupervisorState>>()
            .paths
            .app_log,
        "termination signal",
        message,
    );
}

#[cfg(test)]
mod tests {
    use super::dispatch_termination_once;
    use crate::{ShutdownState, run_shutdown_once};
    use std::{cell::Cell, collections::VecDeque};

    /// 单次可控 SIGTERM 通知只调用一次统一退出 callback。
    #[tokio::test]
    async fn sigterm_dispatches_exit_callback_once() {
        let dispatch_calls = Cell::new(0);

        let received = dispatch_termination_once(std::future::ready(Some(())), || {
            dispatch_calls.set(dispatch_calls.get() + 1);
        })
        .await;

        assert!(received);
        assert_eq!(dispatch_calls.get(), 1);
    }

    /// 一次性 listener 消费首个通知后结束，不读取 fixture 中的第二个信号。
    #[tokio::test]
    async fn listener_stops_before_second_signal() {
        let mut signals = VecDeque::from([(), ()]);
        let dispatch_calls = Cell::new(0);

        let received = dispatch_termination_once(std::future::ready(signals.pop_front()), || {
            dispatch_calls.set(dispatch_calls.get() + 1)
        })
        .await;

        assert!(received);
        assert_eq!(dispatch_calls.get(), 1);
        assert_eq!(signals.len(), 1);
    }

    /// SIGTERM 与后续 UI 退出竞态仍只允许一个 shutdown owner。
    #[tokio::test]
    async fn sigterm_and_ui_share_one_shutdown_owner() {
        let state = ShutdownState::default();
        let shutdown_calls = Cell::new(0);

        dispatch_termination_once(std::future::ready(Some(())), || {
            run_shutdown_once(&state, || {
                shutdown_calls.set(shutdown_calls.get() + 1);
                Ok(())
            })
            .unwrap();
        })
        .await;

        run_shutdown_once(&state, || {
            shutdown_calls.set(shutdown_calls.get() + 1);
            Ok(())
        })
        .unwrap();

        assert_eq!(shutdown_calls.get(), 1);
    }
}
