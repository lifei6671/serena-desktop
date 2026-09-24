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

        // shutdown 失败后 gate 可重试；receiver 存活期间每个 SIGTERM 都重新投递。
        while dispatch_termination(termination.recv(), || {
            dispatch_exit_on_main_thread(app.clone());
        })
        .await
        {}
    });
}

/// 消费单个 SIGTERM 并投递退出 callback；返回值只表示 receiver 是否仍存活。
async fn dispatch_termination(
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
    use super::dispatch_termination;
    use crate::{ShutdownState, run_shutdown_once};
    use std::{cell::Cell, collections::VecDeque};

    /// 单次可控 SIGTERM 通知只调用一次统一退出 callback。
    #[tokio::test]
    async fn sigterm_dispatches_exit_callback_once() {
        let dispatch_calls = Cell::new(0);

        let received = dispatch_termination(std::future::ready(Some(())), || {
            dispatch_calls.set(dispatch_calls.get() + 1);
        })
        .await;

        assert!(received);
        assert_eq!(dispatch_calls.get(), 1);
    }

    /// receiver 中的两个连续 SIGTERM 均可投递，关闭后才停止。
    #[tokio::test]
    async fn listener_dispatches_two_signals() {
        let mut signals = VecDeque::from([(), ()]);
        let dispatch_calls = Cell::new(0);

        while dispatch_termination(std::future::ready(signals.pop_front()), || {
            dispatch_calls.set(dispatch_calls.get() + 1);
        })
        .await
        {}

        assert_eq!(dispatch_calls.get(), 2);
        assert!(signals.is_empty());
    }

    /// 首次 shutdown 失败后，第二个 SIGTERM 可重新取得 shutdown owner。
    #[tokio::test]
    async fn second_sigterm_retries_failed_shutdown() {
        let state = ShutdownState::default();
        let shutdown_calls = Cell::new(0);
        let mut signals = VecDeque::from([(), ()]);

        while dispatch_termination(std::future::ready(signals.pop_front()), || {
            let result = run_shutdown_once(&state, || {
                shutdown_calls.set(shutdown_calls.get() + 1);
                if shutdown_calls.get() == 1 {
                    Err("first shutdown failed".into())
                } else {
                    Ok(())
                }
            });
            if shutdown_calls.get() == 1 {
                assert_eq!(result, Err("first shutdown failed".into()));
            } else {
                assert_eq!(result, Ok(()));
            }
        })
        .await
        {}

        assert_eq!(shutdown_calls.get(), 2);
    }

    /// SIGTERM 与后续 UI 退出竞态仍只允许一个 shutdown owner。
    #[tokio::test]
    async fn sigterm_and_ui_share_one_shutdown_owner() {
        let state = ShutdownState::default();
        let shutdown_calls = Cell::new(0);

        dispatch_termination(std::future::ready(Some(())), || {
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
