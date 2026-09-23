use tokio_util::sync::CancellationToken;

/// 非 Windows 资源池只传播停止信号，不创建 Runtime。
pub(crate) struct CodexRuntimePool {
    pub stop: CancellationToken,
}

impl Default for CodexRuntimePool {
    /// 创建没有 Runtime 所有权的不可用资源池。
    fn default() -> Self {
        Self {
            stop: CancellationToken::new(),
        }
    }
}

impl CodexRuntimePool {
    /// 不可用资源池没有受管 Runtime，也没有需要隔离的失败记录。
    pub(crate) fn check_workspace(&self, _workspace: &str) -> Result<(), String> {
        Ok(())
    }

    /// 该实现从未拥有子进程，关闭时只取消等待任务。
    pub(crate) async fn shutdown(&self) -> Result<(), String> {
        self.stop.cancel();
        Ok(())
    }

    /// 不可用资源池不会保留任何 Runtime。
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::CodexRuntimePool;

    /// 后端不可用不能伪装成 Runtime 隔离失败，运行请求仍由 Provider 边界拒绝。
    #[test]
    fn unavailable_pool_does_not_quarantine_workspace_reads() {
        let pool = CodexRuntimePool::default();
        assert_eq!(pool.check_workspace("/fixture"), Ok(()));
        assert!(pool.is_empty());
    }
}
