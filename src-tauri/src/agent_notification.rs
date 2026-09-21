//! Agent 任务终态的 Desktop 副作用实现。

use crate::{
    agent::notification::{AgentTerminalNotifier, AgentTerminalStatus},
    config::ManagerConfig,
    logs,
    serena::SupervisorState,
};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use tauri::AppHandle;
use tauri_plugin_notification::{NotificationExt, PermissionState};

/// 已由设置与终态共同裁决的提醒动作，便于在不访问系统 API 时验证策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentNotificationPlan {
    pub(crate) title: &'static str,
    pub(crate) body: String,
    pub(crate) send_system_notification: bool,
    pub(crate) play_sound: bool,
}

/// 根据当前设置计算终态提醒；用户主动取消与两个能力均关闭时不产生副作用。
pub(crate) fn plan_agent_notification(
    config: &ManagerConfig,
    terminal: AgentTerminalStatus,
    task_title: &str,
) -> Option<AgentNotificationPlan> {
    let (enabled, title) = match terminal {
        AgentTerminalStatus::Completed => (
            config.agent_success_notification_enabled,
            "Agent 任务已完成",
        ),
        AgentTerminalStatus::Failed | AgentTerminalStatus::Interrupted => (
            config.agent_failure_notification_enabled,
            "Agent 任务已结束",
        ),
        AgentTerminalStatus::Cancelled => return None,
    };
    if !enabled || (!config.agent_system_notification_enabled && !config.agent_sound_enabled) {
        return None;
    }
    Some(AgentNotificationPlan {
        title,
        body: task_title.to_owned(),
        send_system_notification: config.agent_system_notification_enabled,
        play_sound: config.agent_sound_enabled,
    })
}

/// Desktop 终态提醒实现；已计划的 Execution 在当前 Host 生命周期内最多处理一次。
pub(crate) struct DesktopAgentTerminalNotifier {
    app: AppHandle,
    supervisor: Arc<SupervisorState>,
    notified_execution_ids: Mutex<HashSet<String>>,
}

impl DesktopAgentTerminalNotifier {
    /// 绑定 Desktop 产品状态与 Tauri 句柄。
    pub(crate) fn new(app: AppHandle, supervisor: Arc<SupervisorState>) -> Self {
        Self {
            app,
            supervisor,
            notified_execution_ids: Mutex::new(HashSet::new()),
        }
    }

    /// 仅在存在真实提醒计划时占用去重键，取消或全部关闭时不污染后续状态。
    fn claim_notification(&self, execution_id: &str) -> Result<bool, ()> {
        claim_notification(&self.notified_execution_ids, execution_id)
    }
}

/// 为已计划的提醒申请当前 Host 生命周期内的唯一发送权。
fn claim_notification(ids: &Mutex<HashSet<String>>, execution_id: &str) -> Result<bool, ()> {
    ids.lock()
        .map_err(|_| ())
        .map(|mut ids| ids.insert(execution_id.to_owned()))
}

impl AgentTerminalNotifier for DesktopAgentTerminalNotifier {
    /// 执行非关键桌面副作用；系统 API 失败也视为已处理以防重复弹出。
    fn notify(
        &self,
        terminal: AgentTerminalStatus,
        execution_id: &str,
        task_title: &str,
    ) -> Result<(), ()> {
        let config = self.supervisor.snapshot().config;
        let Some(plan) = plan_agent_notification(&config, terminal, task_title) else {
            return Ok(());
        };
        if !self.claim_notification(execution_id)? {
            return Ok(());
        }
        if plan.send_system_notification && send_system_notification(&self.app, &plan).is_err() {
            log_notification_failure(&self.supervisor, "AGENT_NOTIFICATION_SYSTEM_FAILED");
        }
        if plan.play_sound && play_system_sound().is_err() {
            log_notification_failure(&self.supervisor, "AGENT_NOTIFICATION_SOUND_FAILED");
        }
        Ok(())
    }
}

/// 按插件权限状态请求权限，并在获准后发送系统通知。
fn send_system_notification(app: &AppHandle, plan: &AgentNotificationPlan) -> Result<(), ()> {
    let notification = app.notification();
    if notification.permission_state().map_err(|_| ())? != PermissionState::Granted {
        notification.request_permission().map_err(|_| ())?;
    }
    if notification.permission_state().map_err(|_| ())? != PermissionState::Granted {
        return Ok(());
    }
    notification
        .builder()
        .title(plan.title)
        .body(&plan.body)
        .show()
        .map_err(|_| ())
}

/// Windows 使用轻量系统提示音；其他平台暂时安全静默。
#[cfg(windows)]
fn play_system_sound() -> Result<(), ()> {
    if unsafe { windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(0) } == 0 {
        return Err(());
    }
    Ok(())
}

/// 非 Windows 不引入额外音频依赖，保持构建与运行安全。
#[cfg(not(windows))]
fn play_system_sound() -> Result<(), ()> {
    Ok(())
}

/// 副作用错误只记固定安全码，绝不回流影响 Agent 终态。
fn log_notification_failure(supervisor: &SupervisorState, code: &str) {
    logs::append(&supervisor.paths.app_log, "agent notification", code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_policy_maps_outcomes_and_capabilities() {
        let config = ManagerConfig::default();
        let completed =
            plan_agent_notification(&config, AgentTerminalStatus::Completed, "任务标题").unwrap();
        assert_eq!(completed.title, "Agent 任务已完成");
        assert_eq!(completed.body, "任务标题");
        assert!(completed.send_system_notification);
        assert!(completed.play_sound);
        for terminal in [
            AgentTerminalStatus::Failed,
            AgentTerminalStatus::Interrupted,
        ] {
            assert_eq!(
                plan_agent_notification(&config, terminal, "任务标题")
                    .unwrap()
                    .title,
                "Agent 任务已结束"
            );
        }
        assert!(
            plan_agent_notification(&config, AgentTerminalStatus::Cancelled, "任务标题").is_none()
        );
    }

    #[test]
    fn terminal_policy_respects_independent_capability_toggles() {
        let mut config = ManagerConfig {
            agent_system_notification_enabled: false,
            ..Default::default()
        };
        let sound_only =
            plan_agent_notification(&config, AgentTerminalStatus::Completed, "任务标题").unwrap();
        assert!(!sound_only.send_system_notification);
        assert!(sound_only.play_sound);
        config.agent_system_notification_enabled = true;
        config.agent_sound_enabled = false;
        let notification_only =
            plan_agent_notification(&config, AgentTerminalStatus::Completed, "任务标题").unwrap();
        assert!(notification_only.send_system_notification);
        assert!(!notification_only.play_sound);
        config.agent_system_notification_enabled = false;
        assert!(
            plan_agent_notification(&config, AgentTerminalStatus::Completed, "任务标题").is_none()
        );
        config.agent_failure_notification_enabled = false;
        assert!(
            plan_agent_notification(&config, AgentTerminalStatus::Failed, "任务标题").is_none()
        );
    }

    #[test]
    fn deduplication_only_claims_planned_notifications() {
        let ids = Mutex::<HashSet<String>>::new(HashSet::new());
        let config = ManagerConfig::default();
        assert!(
            plan_agent_notification(&config, AgentTerminalStatus::Cancelled, "任务标题").is_none()
        );
        assert!(claim_notification(&ids, "execution-1").unwrap());
        assert!(!claim_notification(&ids, "execution-1").unwrap());
    }
}
