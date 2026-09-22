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

/// macOS 只保存 startup recovery 所需状态，仍不开放执行、继续或取消。
#[cfg(target_os = "macos")]
struct UnavailableCodexProvider {
    store: StateStore,
    owner: String,
}

/// 其他非 Windows 平台保持完全不可用的占位实现。
#[cfg(not(target_os = "macos"))]
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

    /// macOS 仅声明 startup recovery；其他非 Windows 平台所有能力均不可用。
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            can_execute: false,
            can_continue: false,
            can_cancel: false,
            can_recover: cfg!(target_os = "macos"),
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

    /// macOS 调用独立 recovery；其他非 Windows 平台仍返回 unavailable。
    fn startup_reconcile<'a>(
        &'a self,
        _context: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        #[cfg(target_os = "macos")]
        return Box::pin(async move {
            super::macos_recovery::recover_startup(&self.store, &self.owner)
                .await
                .map_err(|_| ProviderError {
                    code: ProviderErrorCode::AgentProviderOperationFailed,
                })
        });
        #[cfg(not(target_os = "macos"))]
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
    // 本阶段不启动或探测 Codex；pool/discovery 只保留与 Windows 注册函数一致的调用契约。
    let _ = (runtime_pool, discovery);
    #[cfg(target_os = "macos")]
    let provider = UnavailableCodexProvider { store, owner };
    #[cfg(not(target_os = "macos"))]
    let provider = {
        let _ = (store, owner);
        UnavailableCodexProvider
    };
    registry.register(Arc::new(provider), ProviderHealth::Unavailable)
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
            assert_eq!(capabilities.can_recover, cfg!(target_os = "macos"));
            match registry.get(&provider_id) {
                Err(error) => assert_eq!(error.code, ProviderErrorCode::AgentProviderUnavailable),
                Ok(_) => panic!("unavailable Codex provider resolved for dispatch"),
            }
        }
    }
}
