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
    fn notify(
        &self,
        status: AgentTerminalStatus,
        execution_id: &str,
        task_title: &str,
    ) -> Result<(), ()>;
}

/// 测试和非桌面路径使用的空实现，保持既有初始化 API 不变。
struct NoopAgentTerminalNotifier;

impl AgentTerminalNotifier for NoopAgentTerminalNotifier {
    fn notify(
        &self,
        _status: AgentTerminalStatus,
        _execution_id: &str,
        _task_title: &str,
    ) -> Result<(), ()> {
        Ok(())
    }
}

/// 生成与前端一致的任务标题：官方 Thread 名优先，其余使用规整后的 Prompt 摘要。
pub(crate) fn task_title(thread_name: Option<&str>, prompt: &str) -> String {
    if let Some(name) = thread_name
        .map(|name| name.trim_matches(is_ecmascript_whitespace))
        .filter(|name| !name.is_empty())
    {
        return name.to_owned();
    }

    let mut summary = String::new();
    let mut pending_space = false;
    for character in prompt.chars() {
        if is_ecmascript_whitespace(character) {
            pending_space = !summary.is_empty();
        } else {
            if pending_space {
                summary.push(' ');
                pending_space = false;
            }
            summary.push(character);
        }
    }
    if summary.is_empty() {
        return "未命名任务".into();
    }

    let mut characters = summary.chars();
    let title: String = characters.by_ref().take(100).collect();
    if characters.next().is_some() {
        format!("{title}…")
    } else {
        title
    }
}

/// 对齐 ECMAScript 的 `\s` 与 `trim` 白名单，确保 Desktop 与前端摘要语义一致。
fn is_ecmascript_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'..='\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

/// 构造不产生副作用的默认终态提醒端口。
pub(crate) fn noop_agent_terminal_notifier() -> Arc<dyn AgentTerminalNotifier> {
    Arc::new(NoopAgentTerminalNotifier)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 持久化 Thread 名存在时，通知标题与前端一样优先使用其规整值。
    #[test]
    fn task_title_prefers_trimmed_thread_name() {
        assert_eq!(
            task_title(Some("\u{FEFF}  已命名任务\u{3000}"), "应被忽略"),
            "已命名任务"
        );
    }

    /// Prompt 回退折叠 ECMAScript 空白，并按 Unicode 标量值而非字节截断。
    #[test]
    fn task_title_normalizes_and_bounds_prompt_fallback() {
        assert_eq!(
            task_title(None, "\u{FEFF}  第一段\t\n第二段\u{3000}第三段  "),
            "第一段 第二段 第三段"
        );
        assert_eq!(
            task_title(None, &format!("{}尾部", "界".repeat(100))),
            format!("{}…", "界".repeat(100))
        );
    }

    /// 无可展示 Thread 名和 Prompt 时保留稳定的未命名任务占位。
    #[test]
    fn task_title_uses_unnamed_fallback() {
        assert_eq!(task_title(Some("\u{FEFF}\u{3000}"), "\t\n"), "未命名任务");
    }
}
