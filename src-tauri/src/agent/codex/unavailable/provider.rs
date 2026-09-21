//! 非 Windows Codex Provider 显式不可用实现。

use crate::agent::{
    provider::{
        ProviderCancelContext, ProviderCapabilities, ProviderDescriptor, ProviderError,
        ProviderErrorCode, ProviderExecutionContext, ProviderId, ProviderStartupContext,
        port::{
            AgentEventSink, AgentProvider, ProviderAcceptanceSink, ProviderExecutionFailure,
            ProviderFuture, ProviderReconcileSummary,
        },
        registry::{ProviderHealth, ProviderRegistry},
    },
    store::StateStore,
};
use std::{path::PathBuf, sync::Arc};

/// 非 Windows 平台的 Codex Provider 占位实现，只暴露稳定的不可用契约。
struct UnavailableCodexProvider;

impl AgentProvider for UnavailableCodexProvider {
    /// 保留 Codex 身份，非 Windows 后端不声明版本。
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new("codex".into()).expect("static Codex provider id is valid"),
            display_name: "Codex".into(),
            version: None,
        }
    }

    /// 所有 Codex Runtime 能力在非 Windows 平台均不可用。
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            can_execute: false,
            can_continue: false,
            can_cancel: false,
            can_recover: false,
            activity: false,
            token_usage: false,
        }
    }

    /// 调度请求始终以稳定的 Provider 不可用状态失败。
    fn execute<'a>(
        &'a self,
        _context: ProviderExecutionContext,
        _acceptance: Arc<dyn ProviderAcceptanceSink>,
        _telemetry: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<
        'a,
        Result<crate::agent::provider::ProviderRunResult, ProviderExecutionFailure>,
    > {
        Box::pin(async {
            Err(ProviderExecutionFailure::State(
                "AGENT_PROVIDER_UNAVAILABLE".into(),
            ))
        })
    }

    /// 取消请求不能到达不存在的 Codex 后端。
    fn cancel<'a>(
        &'a self,
        _context: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>> {
        Box::pin(async {
            Err(ProviderError {
                code: ProviderErrorCode::AgentProviderUnavailable,
            })
        })
    }

    /// 启动恢复不能到达不存在的 Codex 后端。
    fn startup_reconcile<'a>(
        &'a self,
        _context: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        Box::pin(async {
            Err(ProviderError {
                code: ProviderErrorCode::AgentProviderUnavailable,
            })
        })
    }
}

/// 以固定不可用健康状态注册非 Windows Codex Provider，保持跨平台调用契约一致。
pub(crate) fn register_codex_provider_with_discovery(
    registry: &mut ProviderRegistry,
    store: StateStore,
    owner: String,
    runtime_pool: Arc<super::pool::CodexRuntimePool>,
    discovery: Result<PathBuf, String>,
) -> Result<(), ProviderError> {
    // 非 Windows 不启动或探测 Codex；四个输入仅保留与 Windows 注册函数一致的调用契约。
    let _ = (store, owner, runtime_pool, discovery);
    registry.register(
        Arc::new(UnavailableCodexProvider),
        ProviderHealth::Unavailable,
    )
}

#[cfg(test)]
mod tests {
    use super::register_codex_provider_with_discovery;
    use crate::agent::{
        codex::pool::CodexRuntimePool,
        provider::{ProviderErrorCode, ProviderId, registry::ProviderRegistry},
        store::StateStore,
    };
    use std::{path::PathBuf, sync::Arc};

    /// 验证无论 discovery 成功或失败，Codex 都注册为不可调度的 Provider。
    #[tokio::test]
    async fn unavailable_provider_is_registered_but_cannot_be_dispatched() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().to_owned()).await.unwrap();

        for discovery in [
            Ok(PathBuf::from("fixture")),
            Err("BACKEND_UNAVAILABLE: fixture".into()),
        ] {
            let mut registry = ProviderRegistry::new();
            register_codex_provider_with_discovery(
                &mut registry,
                store.clone(),
                "test-owner".into(),
                Arc::new(CodexRuntimePool::default()),
                discovery,
            )
            .unwrap();

            let provider_id = ProviderId::new("codex".into()).unwrap();
            let capabilities = registry
                .get_registered(&provider_id)
                .unwrap()
                .capabilities();
            assert!(!capabilities.can_execute);
            assert!(!capabilities.can_continue);
            assert!(!capabilities.can_cancel);
            assert!(!capabilities.can_recover);
            match registry.get(&provider_id) {
                Err(error) => assert_eq!(error.code, ProviderErrorCode::AgentProviderUnavailable),
                Ok(_) => panic!("unavailable Codex provider resolved for dispatch"),
            }
        }
    }
}
