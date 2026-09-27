//! capability-conservative CodeBuddy Provider skeleton。

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

/// CB6-001 只注册 presence 与 Admission Health，不持有 Runtime 或 StateStore。
pub(crate) struct CodeBuddyProvider {
    discovery: Option<DiscoveryResult>,
    discovery_error: Option<DiscoveryError>,
}

impl CodeBuddyProvider {
    /// 从一次无进程 discovery 构造 Provider，失败结果仍保留 registered skeleton。
    fn from_discovery(discovery: Result<DiscoveryResult, DiscoveryError>) -> Self {
        match discovery {
            Ok(discovery) => Self {
                discovery: Some(discovery),
                discovery_error: None,
            },
            Err(error) => Self {
                discovery: None,
                discovery_error: Some(error),
            },
        }
    }

    /// 返回本机 resolved LaunchSpec；当前任务不会消费它启动进程。
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
) -> Result<(), ProviderError> {
    register_codebuddy_provider_with_discovery(registry, super::discover())
}

/// 将已冻结 discovery 结果注册为 Available/Unavailable Provider。
pub(crate) fn register_codebuddy_provider_with_discovery(
    registry: &mut ProviderRegistry,
    discovery: Result<DiscoveryResult, DiscoveryError>,
) -> Result<(), ProviderError> {
    let health = if discovery.is_ok() {
        ProviderHealth::Available
    } else {
        ProviderHealth::Unavailable
    };
    registry.register(
        Arc::new(CodeBuddyProvider::from_discovery(discovery)),
        health,
    )
}

/// 构造统一的 capability unsupported 错误，避免 skeleton 进入任何生命周期路径。
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

    /// CB6-001 没有生产 ACP/Runtime 证据，所有能力保持 false。
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

    /// skeleton 不得创建 Runtime、Session、Execution 或 Claim。
    fn execute<'a>(
        &'a self,
        _context: ProviderExecutionContext,
        _acceptance: Arc<dyn ProviderAcceptanceSink>,
        _telemetry: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>> {
        Box::pin(async {
            Err(ProviderExecutionFailure::State(
                "AGENT_PROVIDER_CAPABILITY_UNSUPPORTED".into(),
            ))
        })
    }

    /// CB6-001 没有 Cancel 实现，直接返回稳定公共错误。
    fn cancel<'a>(
        &'a self,
        _context: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>> {
        Box::pin(async { Err(unsupported()) })
    }

    /// CB6-001 没有 Recovery，实现保持 fail closed 且不会访问 StateStore。
    fn startup_reconcile<'a>(
        &'a self,
        _context: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        Box::pin(async { Err(unsupported()) })
    }
}

#[cfg(test)]
mod tests;
