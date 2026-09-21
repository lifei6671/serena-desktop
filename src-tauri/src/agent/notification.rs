//! Agent core 的终态提醒端口，不依赖任何具体 Provider 或桌面 API。

use std::sync::Arc;

/// 已成功持久化的业务终态；不承载 Provider 的私有结果语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentTerminalStatus {
    Completed,
    Failed,
    Interrupted,
    Cancelled,
}

impl AgentTerminalStatus {
    /// 将持久化 Execution 状态限制为可提醒的业务终态。
    pub(crate) fn from_persisted_status(status: &str) -> Option<Self> {
        match status {
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "interrupted" => Some(Self::Interrupted),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// 产品层终态副作用端口；实现失败绝不改变 Agent 生命周期结果。
pub(crate) trait AgentTerminalNotifier: Send + Sync {
    fn notify(&self, status: AgentTerminalStatus, execution_id: &str) -> Result<(), ()>;
}

/// 测试和非桌面路径使用的空实现，保持既有初始化 API 不变。
struct NoopAgentTerminalNotifier;

impl AgentTerminalNotifier for NoopAgentTerminalNotifier {
    fn notify(&self, _status: AgentTerminalStatus, _execution_id: &str) -> Result<(), ()> {
        Ok(())
    }
}

/// 构造不产生副作用的默认终态提醒端口。
pub(crate) fn noop_agent_terminal_notifier() -> Arc<dyn AgentTerminalNotifier> {
    Arc::new(NoopAgentTerminalNotifier)
}
