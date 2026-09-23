use crate::{
    agent::product::AgentProductService,
    commands, logs,
    remote::{RemoteAccessMode, SelfHostedProvider, Status},
    serena::SupervisorState,
};
use std::{sync::Arc, time::Duration};
use tauri::{
    AppHandle, Emitter, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const TRAY_REFRESH_INTERVAL: Duration = Duration::from_millis(1500);

/// 持有只读状态项句柄；业务状态仍由各自 authority 管理。
#[derive(Clone)]
struct TrayController {
    broker: MenuItem<tauri::Wry>,
    remote: MenuItem<tauri::Wry>,
    agent: MenuItem<tauri::Wry>,
}

impl TrayController {
    /// 从 Broker、Remote 与 Product authority 读取快照并更新菜单文本。
    async fn refresh(&self, app: &AppHandle) {
        let broker = crate::mcp::get(app);
        let broker_snapshot = broker.snapshot().await;
        let remote_snapshot = broker.remote.snapshot();
        let config = broker.config();

        let _ = self.broker.set_text(format_broker_status(
            broker_snapshot.running,
            broker_snapshot.port,
            broker_snapshot.last_error.is_some(),
        ));
        let _ = self.remote.set_text(format_remote_status(
            remote_snapshot.mode,
            remote_snapshot.config.self_hosted.provider,
            remote_snapshot.status,
            broker_snapshot.running,
        ));

        if config.agent_enabled {
            let product = app.state::<Arc<AgentProductService>>();
            // 查询失败时保留上次文本，避免托盘刷新干扰 Host 主流程。
            if let Ok(count) = product.nonterminal_execution_count().await {
                let _ = self.agent.set_text(format_agent_status(true, Some(count)));
            }
        } else {
            let _ = self.agent.set_text(format_agent_status(false, None));
        }
    }
}

/// 创建跨平台共用的 Host 状态、导航与退出菜单。
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let broker_authority = crate::mcp::get(app);
    let config = broker_authority.config();
    let remote_snapshot = broker_authority.remote.snapshot();
    let title = MenuItem::with_id(app, "title", "Serena Desktop", false, None::<&str>)?;
    let broker = MenuItem::with_id(
        app,
        "broker-status",
        format_broker_status(false, config.broker.port, false),
        false,
        None::<&str>,
    )?;
    let remote = MenuItem::with_id(
        app,
        "remote-status",
        format_remote_status(
            remote_snapshot.mode,
            remote_snapshot.config.self_hosted.provider,
            remote_snapshot.status,
            false,
        ),
        false,
        None::<&str>,
    )?;
    let agent = MenuItem::with_id(
        app,
        "agent-status",
        format_agent_status(config.agent_enabled, None),
        false,
        None::<&str>,
    )?;
    let show = MenuItem::with_id(app, "show", "打开 Serena Desktop", true, None::<&str>)?;
    let agent_tasks = MenuItem::with_id(app, "agent", "Agent 任务", true, None::<&str>)?;
    let remote_access = MenuItem::with_id(app, "remote", "远程访问", true, None::<&str>)?;
    let logs_item = MenuItem::with_id(app, "logs", "打开日志目录", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Serena Desktop", true, None::<&str>)?;
    let separator_one = PredefinedMenuItem::separator(app)?;
    let separator_two = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &title,
            &broker,
            &remote,
            &agent,
            &separator_one,
            &show,
            &agent_tasks,
            &remote_access,
            &logs_item,
            &separator_two,
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
            id @ ("agent" | "remote") => navigate_from_tray(app, id),
            "logs" => {
                if let Err(error) = commands::open_log_directory(app.clone()) {
                    log_tray_error(app, &error);
                }
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

    let controller = TrayController {
        broker,
        remote,
        agent,
    };
    let refresh_app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            controller.refresh(&refresh_app).await;
            tokio::time::sleep(TRAY_REFRESH_INTERVAL).await;
        }
    });
    Ok(())
}

/// 将 Broker 快照投影为不含内部实现细节的只读文本。
fn format_broker_status(running: bool, port: u16, has_error: bool) -> String {
    if running {
        format!("MCP 连接入口 · 运行中 · {port}")
    } else if has_error {
        "MCP 连接入口 · 启动异常".into()
    } else {
        "MCP 连接入口 · 已停止".into()
    }
}

/// 将 Remote authority 的模式与状态投影为安全、稳定的托盘文本。
fn format_remote_status(
    mode: RemoteAccessMode,
    provider: SelfHostedProvider,
    status: Status,
    broker_running: bool,
) -> String {
    let mode_label = match mode {
        RemoteAccessMode::McpOnly => "仅 MCP",
        RemoteAccessMode::QuickTunnel => "快捷隧道",
        RemoteAccessMode::SelfHostedOAuth => match provider {
            SelfHostedProvider::CustomHttps => "自建 HTTPS",
            SelfHostedProvider::Ngrok => "ngrok",
            SelfHostedProvider::TailscaleFunnel => "Tailscale Funnel",
        },
    };
    let status_label = match status {
        Status::Error | Status::Disconnected => "异常",
        Status::Starting | Status::Installing | Status::DiscoveringUrl | Status::Verifying => {
            "连接中"
        }
        Status::Ready => "已连接",
        Status::Stopping => "正在停止",
        Status::Stopped if mode == RemoteAccessMode::McpOnly && broker_running => "已启用",
        Status::Stopped => "未启用",
    };
    format!("远程访问 · {mode_label} · {status_label}")
}

/// 将 Agent 配置与非终态 Execution 数量投影为 Host 级摘要。
fn format_agent_status(enabled: bool, count: Option<usize>) -> String {
    if !enabled {
        "Agent · 未启用".into()
    } else {
        match count {
            Some(0) => "Agent · 空闲".into(),
            Some(count) => format!("Agent · {count} 个任务执行中"),
            None => "Agent · 已启用".into(),
        }
    }
}

/// 将菜单 ID 映射为前端已有 tab；未知 ID 不产生事件。
fn navigation_target(id: &str) -> Option<&'static str> {
    match id {
        "agent" => Some("agent"),
        "remote" => Some("remote"),
        _ => None,
    }
}

/// 先恢复主窗口，再发出轻量导航事件。
fn navigate_from_tray(app: &AppHandle, id: &str) {
    let Some(target) = navigation_target(id) else {
        return;
    };
    show_main_window(app);
    if let Err(error) = app.emit("tray:navigate", target) {
        log_tray_error(app, &format!("托盘导航通知失败：{error}"));
    }
}

/// 显示、取消最小化并聚焦主窗口。
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 将托盘动作错误写入用户可打开的应用日志。
fn log_tray_error(app: &AppHandle, error: &str) {
    let path = app.state::<Arc<SupervisorState>>().paths.app_log.clone();
    logs::append(&path, "tray", error);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_status_distinguishes_running_stopped_and_start_error() {
        assert_eq!(
            format_broker_status(true, 19120, true),
            "MCP 连接入口 · 运行中 · 19120"
        );
        assert_eq!(
            format_broker_status(false, 19120, false),
            "MCP 连接入口 · 已停止"
        );
        assert_eq!(
            format_broker_status(false, 19120, true),
            "MCP 连接入口 · 启动异常"
        );
    }

    #[test]
    fn remote_status_preserves_mode_and_safe_connection_state() {
        assert_eq!(
            format_remote_status(
                RemoteAccessMode::McpOnly,
                SelfHostedProvider::CustomHttps,
                Status::Stopped,
                false,
            ),
            "远程访问 · 仅 MCP · 未启用"
        );
        assert_eq!(
            format_remote_status(
                RemoteAccessMode::McpOnly,
                SelfHostedProvider::CustomHttps,
                Status::Stopped,
                true,
            ),
            "远程访问 · 仅 MCP · 已启用"
        );
        assert_eq!(
            format_remote_status(
                RemoteAccessMode::QuickTunnel,
                SelfHostedProvider::CustomHttps,
                Status::DiscoveringUrl,
                true,
            ),
            "远程访问 · 快捷隧道 · 连接中"
        );
        assert_eq!(
            format_remote_status(
                RemoteAccessMode::SelfHostedOAuth,
                SelfHostedProvider::Ngrok,
                Status::Ready,
                true,
            ),
            "远程访问 · ngrok · 已连接"
        );
        assert_eq!(
            format_remote_status(
                RemoteAccessMode::SelfHostedOAuth,
                SelfHostedProvider::CustomHttps,
                Status::Error,
                false,
            ),
            "远程访问 · 自建 HTTPS · 异常"
        );
    }

    #[test]
    fn agent_status_uses_nonterminal_execution_count() {
        assert_eq!(format_agent_status(false, None), "Agent · 未启用");
        assert_eq!(format_agent_status(true, None), "Agent · 已启用");
        assert_eq!(format_agent_status(true, Some(0)), "Agent · 空闲");
        assert_eq!(format_agent_status(true, Some(3)), "Agent · 3 个任务执行中");
    }

    #[test]
    fn tray_navigation_only_maps_supported_tabs() {
        assert_eq!(navigation_target("agent"), Some("agent"));
        assert_eq!(navigation_target("remote"), Some("remote"));
        assert_eq!(navigation_target("dashboard"), None);
    }
}
