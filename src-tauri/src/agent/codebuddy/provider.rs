//! CodeBuddy Windows Fresh Execute 与历史 Runtime recovery；能力由已验证的平台边界声明。

use std::sync::Arc;

use crate::agent::provider::{
    ProviderCancelContext, ProviderCapabilities, ProviderDescriptor, ProviderError,
    ProviderErrorCode, ProviderExecutionContext, ProviderId, ProviderRunResult,
    ProviderStartupContext,
    port::{
        AgentEventSink, AgentProvider, ProviderAcceptanceSink, ProviderExecutionFailure,
        ProviderFuture, ProviderReconcileSummary,
    },
    registry::{ProviderHealth, ProviderRegistry},
};

use super::discovery::{DiscoveryError, DiscoveryProvenance, DiscoveryResult, ResolvedLaunchSpec};

/// Release-owned 默认启动描述；它不是本机 resolved LaunchSpec，也不进入用户配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DefaultLaunchDescriptor {
    pub(crate) command: &'static str,
    pub(crate) args: &'static [&'static str],
}

/// Catalog/release 只声明公开 CLI 入口；本机 resolver 决定最终 executable/argv。
pub(crate) const DEFAULT_LAUNCH_DESCRIPTOR: DefaultLaunchDescriptor = DefaultLaunchDescriptor {
    command: "codebuddy",
    args: &["--acp"],
};

/// Discovery 决定 Admission Health；独立 Store/Host authority 始终可恢复历史 Runtime。
pub(crate) struct CodeBuddyProvider {
    store: crate::agent::store::StateStore,
    owner: String,
    discovery: Option<DiscoveryResult>,
    discovery_error: Option<DiscoveryError>,
}

impl CodeBuddyProvider {
    /// 内部准备入口复用生产 fresh primitive，供独立准备与 crash-window 测试。
    #[cfg(windows)]
    #[allow(dead_code, reason = "内部准备入口供独立 lifecycle 验证")]
    pub(crate) async fn prepare_fresh(
        &self,
        execution_id: String,
        desired: super::fresh::DesiredConfiguration,
    ) -> Result<super::fresh::PreparedFreshSession, super::protocol::Failure> {
        super::fresh::prepare(
            self.store.clone(),
            self.owner.clone(),
            execution_id,
            self.resolved_launch_spec()
                .ok_or(super::protocol::Failure::Launch)?,
            desired,
            super::protocol::Limits::default(),
        )
        .await
    }

    /// 从一次无进程 discovery 构造 Provider，失败结果仍保留 registered skeleton。
    pub(super) fn from_discovery(
        store: crate::agent::store::StateStore,
        owner: String,
        discovery: Result<DiscoveryResult, DiscoveryError>,
    ) -> Self {
        match discovery {
            Ok(discovery) => Self {
                store,
                owner,
                discovery: Some(discovery),
                discovery_error: None,
            },
            Err(error) => Self {
                store,
                owner,
                discovery: None,
                discovery_error: Some(error),
            },
        }
    }

    /// 返回当前 registered adapter 冻结的 LaunchSpec，execute 不重新 discovery 或 fallback。
    pub(crate) fn resolved_launch_spec(&self) -> Option<&ResolvedLaunchSpec> {
        self.discovery
            .as_ref()
            .map(|discovery| &discovery.launch_spec)
    }

    /// 返回安全 provenance，供后续本地诊断投影使用。
    pub(crate) fn discovery_provenance(&self) -> Option<&DiscoveryProvenance> {
        self.discovery
            .as_ref()
            .map(|discovery| &discovery.provenance)
    }

    /// 返回稳定 discovery failure；不暴露原始 PATH 或环境。
    pub(crate) fn discovery_error(&self) -> Option<&DiscoveryError> {
        self.discovery_error.as_ref()
    }
}

/// 使用当前系统 resolver 注册 CodeBuddy；缺失不会阻断 Registry bootstrap。
pub(crate) fn register_codebuddy_provider(
    registry: &mut ProviderRegistry,
    store: crate::agent::store::StateStore,
    owner: String,
) -> Result<(), ProviderError> {
    register_codebuddy_provider_with_discovery(registry, store, owner, super::discover())
}

/// 将已冻结 discovery 结果注册为 Available/Unavailable Provider。
pub(crate) fn register_codebuddy_provider_with_discovery(
    registry: &mut ProviderRegistry,
    store: crate::agent::store::StateStore,
    owner: String,
    discovery: Result<DiscoveryResult, DiscoveryError>,
) -> Result<(), ProviderError> {
    let health = if discovery.is_ok() {
        ProviderHealth::Available
    } else {
        ProviderHealth::Unavailable
    };
    registry.register(
        Arc::new(CodeBuddyProvider::from_discovery(store, owner, discovery)),
        health,
    )
}

/// Cancel 尚未实现，保持稳定 capability unsupported。
fn unsupported() -> ProviderError {
    ProviderError {
        code: ProviderErrorCode::AgentProviderCapabilityUnsupported,
    }
}

impl AgentProvider for CodeBuddyProvider {
    /// descriptor 只投影身份与 best-effort 产品版本，不携带 command/args authority。
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new("codebuddy".into()).expect("static CodeBuddy provider id is valid"),
            display_name: "CodeBuddy".into(),
            version: self
                .discovery
                .as_ref()
                .and_then(|discovery| discovery.metadata.product_version.clone()),
        }
    }

    /// Windows Fresh/Activity/Job recovery 已通过 Gate；其他平台与未实现能力保持关闭。
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            can_execute: cfg!(windows),
            can_continue: false,
            can_cancel: false,
            can_recover: cfg!(windows),
            activity: cfg!(windows),
            token_usage: false,
        }
    }

    /// 当前 registered LaunchSpec 驱动受管 fresh lifecycle，接受后由原 owner 完成安全收敛。
    fn execute<'a>(
        &'a self,
        context: ProviderExecutionContext,
        acceptance: Arc<dyn ProviderAcceptanceSink>,
        telemetry: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>> {
        #[cfg(windows)]
        {
            let store = self.store.clone();
            let owner = self.owner.clone();
            let resolved = self.resolved_launch_spec().cloned();
            Box::pin(async move {
                let resolved = resolved.ok_or_else(|| {
                    ProviderExecutionFailure::State("CODEBUDDY_ACP_LAUNCH_FAILED".into())
                })?;
                let (cancel, cancelled) = tokio::sync::oneshot::channel();
                // 一个有界 owner task 持有整个 lifecycle；caller drop 通知它停止 prompt 并收敛证据。
                let task = tokio::spawn(super::execute::run(
                    store,
                    owner,
                    resolved,
                    context.execution_id,
                    acceptance,
                    telemetry,
                    cancelled,
                ));
                let result = task.await.map_err(|_| {
                    ProviderExecutionFailure::State("CODEBUDDY_EXECUTION_OWNER_FAILED".into())
                })?;
                drop(cancel);
                result
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (context, acceptance, telemetry);
            Box::pin(async {
                Err(ProviderExecutionFailure::State(
                    "AGENT_PROVIDER_CAPABILITY_UNSUPPORTED".into(),
                ))
            })
        }
    }

    /// CB6-001 没有 Cancel 实现，直接返回稳定公共错误。
    fn cancel<'a>(
        &'a self,
        _context: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>> {
        Box::pin(async { Err(unsupported()) })
    }

    /// 恢复只依赖 durable ownership，与 CLI presence、enabled 和 admission health 无关。
    fn startup_reconcile<'a>(
        &'a self,
        _context: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        Box::pin(async {
            super::recovery::startup(&self.store, &self.owner)
                .await
                .map_err(|_| ProviderError {
                    code: ProviderErrorCode::AgentProviderOperationFailed,
                })
        })
    }
}

#[cfg(test)]
mod tests;
