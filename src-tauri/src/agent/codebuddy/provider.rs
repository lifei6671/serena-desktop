//! CodeBuddy 跨平台 Fresh Execute 与历史 Runtime recovery；能力由已验证的平台边界声明。

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use crate::agent::provider::{
    ProviderCancelContext, ProviderCapabilities, ProviderConfigurationCatalogContext,
    ProviderDescriptor, ProviderError, ProviderErrorCode, ProviderExecutionContext, ProviderId,
    ProviderRunResult, ProviderStartupContext,
    port::{
        AgentEventSink, AgentProvider, ProviderAcceptanceSink, ProviderContinuationContext,
        ProviderContinuationDecision, ProviderExecutionFailure, ProviderFuture,
        ProviderReconcileSummary,
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
    admission_diagnostic: Arc<RuntimeAdmissionDiagnostic>,
    /// 独立展示 metadata；更新不触碰 discovery/admission authority。
    product_version: Arc<Mutex<Option<String>>>,
    #[cfg(test)]
    test_limits: Option<super::protocol::Limits>,
}

/// adapter-local runtime 诊断；只接受 typed deterministic incompatibility。
#[derive(Default)]
pub(super) struct RuntimeAdmissionDiagnostic(Mutex<Option<&'static str>>);

impl RuntimeAdmissionDiagnostic {
    /// Provider-owned classifier 是写入全局 admission override 的唯一入口。
    pub(super) fn record(&self, failure: super::protocol::Failure) {
        if failure.health_change() == Some(ProviderHealth::Unavailable) {
            *self.0.lock().unwrap() = Some(failure.code());
        }
    }

    /// 返回安全稳定码，不公开 wire、stderr 或远端错误。
    fn code(&self) -> Option<String> {
        self.0.lock().unwrap().map(str::to_owned)
    }
}

impl CodeBuddyProvider {
    /// 使用非持久化受管 Runtime 查询一次 ACP Session 目录，并在返回前完整关闭。
    #[cfg(any(windows, target_os = "macos"))]
    async fn read_configuration_catalog(
        &self,
        context: ProviderConfigurationCatalogContext,
    ) -> Result<crate::agent::provider::ExecutionConfigurationCatalog, ProviderError> {
        use super::{
            fresh::SessionCatalog,
            platform_launcher::{LaunchRequest, UncCurrentDirectoryPolicy},
            protocol::{Failure, Limits},
            runtime::Runtime,
        };
        use agent_client_protocol::schema::v1::NewSessionRequest;
        let resolved = self.resolved_launch_spec().ok_or(ProviderError {
            code: ProviderErrorCode::AgentProviderUnavailable,
        })?;
        let runtime_id = format!(
            "codebuddy-catalog-{}",
            super::store::new_conversation_id().map_err(|_| ProviderError {
                code: ProviderErrorCode::AgentProviderOperationFailed,
            })?
        );
        let request = LaunchRequest::from_resolved(
            resolved,
            Path::new(&context.cwd),
            UncCurrentDirectoryPolicy::Unsupported,
            runtime_id,
        )
        .map_err(|_| ProviderError {
            code: ProviderErrorCode::AgentProviderOperationFailed,
        })?;
        let cwd = request.projected_cwd().as_path().to_owned();
        let (runtime, _) = Runtime::start(request, Limits::default())
            .await
            .map_err(catalog_failure)?;
        let result = async {
            let requests = &runtime.client.as_ref().ok_or(Failure::Closed)?.requests;
            let response = requests.request(NewSessionRequest::new(cwd)).await?;
            let session_id = response.session_id.to_string();
            let extensions = requests.shared.take_session_new_extensions(&session_id)?;
            requests.shared.register_route(&session_id)?;
            let frames = requests.shared.take_session(&session_id)?;
            let mut catalog = SessionCatalog {
                response,
                models: extensions.models,
            };
            catalog.replay(&session_id, &frames)?;
            catalog.configuration_catalog_for_provider(requests).await
        }
        .await;
        let cleanup = runtime.shutdown().await;
        cleanup.map_err(catalog_failure)?;
        result.map_err(catalog_failure)
    }

    /// 内部准备入口复用生产 fresh primitive，供独立准备与 crash-window 测试。
    #[cfg(any(windows, target_os = "macos"))]
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

    /// 静态 discovery 冻结准入事实；macOS 另开有界后台版本 probe。
    pub(super) fn from_discovery(
        store: crate::agent::store::StateStore,
        owner: String,
        discovery: Result<DiscoveryResult, DiscoveryError>,
    ) -> Self {
        let product_version = Arc::new(Mutex::new(
            discovery
                .as_ref()
                .ok()
                .and_then(|value| value.metadata.product_version.clone()),
        ));
        #[cfg(target_os = "macos")]
        if let Ok(discovery) = &discovery
            && discovery.metadata.product_version.is_none()
        {
            let resolved = discovery.launch_spec.clone();
            let metadata = product_version.clone();
            // 每个注册实例只 probe 一次；catalog poll 读取此实例的更新，不启动新进程。
            let _ = std::thread::Builder::new()
                .name("codebuddy-version".into())
                .spawn(move || {
                    let version = super::runtime::probe_product_version(
                        &resolved,
                        std::time::Duration::from_secs(3),
                    );
                    *metadata.lock().unwrap() = version;
                });
        }
        match discovery {
            Ok(discovery) => Self {
                store,
                owner,
                discovery: Some(discovery),
                discovery_error: None,
                product_version,
                admission_diagnostic: Arc::new(RuntimeAdmissionDiagnostic::default()),
                #[cfg(test)]
                test_limits: None,
            },
            Err(error) => Self {
                store,
                owner,
                discovery: None,
                discovery_error: Some(error),
                product_version,
                admission_diagnostic: Arc::new(RuntimeAdmissionDiagnostic::default()),
                #[cfg(test)]
                test_limits: None,
            },
        }
    }

    #[cfg(test)]
    pub(super) fn with_limits_for_test(mut self, limits: super::protocol::Limits) -> Self {
        self.test_limits = Some(limits);
        self
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

/// 未实现的平台没有受管 Cancel runtime 能力。
#[cfg(not(any(windows, target_os = "macos")))]
fn unsupported() -> ProviderError {
    ProviderError {
        code: ProviderErrorCode::AgentProviderCapabilityUnsupported,
    }
}

/// 将配置目录失败压缩为稳定 Provider 错误，不暴露 ACP payload。
#[cfg(any(windows, target_os = "macos"))]
fn catalog_failure(failure: super::protocol::Failure) -> ProviderError {
    let code = if matches!(
        failure,
        super::protocol::Failure::Incompatible
            | super::protocol::Failure::Malformed
            | super::protocol::Failure::Configuration
    ) {
        ProviderErrorCode::AgentProviderContractError
    } else {
        ProviderErrorCode::AgentProviderOperationFailed
    };
    ProviderError { code }
}

impl AgentProvider for CodeBuddyProvider {
    /// descriptor 只投影身份与 best-effort 产品版本，不携带 command/args authority。
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new("codebuddy".into()).expect("static CodeBuddy provider id is valid"),
            display_name: "CodeBuddy".into(),
            version: self.product_version.lock().unwrap().clone(),
            // 与 ManagedClient initialize 要求并校验的 ProtocolVersion::V1 契约一致。
            protocol: Some("ACP v1".into()),
        }
    }

    /// Windows Job 与 macOS process-group 共用 Fresh/Continue/Activity/Recovery/Cancel。
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            can_execute: cfg!(any(windows, target_os = "macos")),
            can_continue: cfg!(any(windows, target_os = "macos")),
            can_cancel: cfg!(any(windows, target_os = "macos")),
            can_recover: cfg!(any(windows, target_os = "macos")),
            activity: cfg!(any(windows, target_os = "macos")),
            token_usage: false,
        }
    }

    /// 配置目录复用正式 launcher/ACP guard，但没有 Execution 或 Claim authority。
    fn configuration_catalog<'a>(
        &'a self,
        context: ProviderConfigurationCatalogContext,
    ) -> ProviderFuture<
        'a,
        Result<crate::agent::provider::ExecutionConfigurationCatalog, ProviderError>,
    > {
        #[cfg(any(windows, target_os = "macos"))]
        {
            Box::pin(async move { self.read_configuration_catalog(context).await })
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = context;
            Box::pin(async { Err(unsupported()) })
        }
    }

    /// 只有受管 initialize 已证明的确定性不兼容会阻断后续新执行。
    fn admission_diagnostic(&self) -> Option<String> {
        self.admission_diagnostic.code()
    }

    /// generic core/admission 保持 authority；这里只验证 source exact CodeBuddy private S1。
    fn validate_continuation<'a>(
        &'a self,
        context: ProviderContinuationContext,
    ) -> ProviderFuture<'a, Result<ProviderContinuationDecision, ProviderError>> {
        #[cfg(any(windows, target_os = "macos"))]
        {
            Box::pin(async move {
                let source = super::store::CodeBuddyStore(self.store.clone())
                    .continuation_source(context.source_execution_id)
                    .await
                    .map_err(|_| ProviderError {
                        code: ProviderErrorCode::AgentProviderOperationFailed,
                    })?;
                Ok(if source.is_some() {
                    ProviderContinuationDecision::Eligible
                } else {
                    ProviderContinuationDecision::Ineligible
                })
            })
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = context;
            Box::pin(async { Ok(ProviderContinuationDecision::Ineligible) })
        }
    }

    /// 当前 registered LaunchSpec 驱动受管 fresh lifecycle，接受后由原 owner 完成安全收敛。
    fn execute<'a>(
        &'a self,
        context: ProviderExecutionContext,
        acceptance: Arc<dyn ProviderAcceptanceSink>,
        telemetry: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>> {
        #[cfg(any(windows, target_os = "macos"))]
        {
            let store = self.store.clone();
            let owner = self.owner.clone();
            let resolved = self.resolved_launch_spec().cloned();
            let admission_diagnostic = self.admission_diagnostic.clone();
            #[cfg(test)]
            let limits = self.test_limits.unwrap_or_default();
            #[cfg(not(test))]
            let limits = super::protocol::Limits::default();
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
                    super::execute::RunControl {
                        cancelled,
                        admission_diagnostic,
                        limits,
                    },
                ));
                let result = task.await.map_err(|_| {
                    ProviderExecutionFailure::State("CODEBUDDY_EXECUTION_OWNER_FAILED".into())
                })?;
                drop(cancel);
                result
            })
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = (context, acceptance, telemetry);
            Box::pin(async {
                Err(ProviderExecutionFailure::State(
                    "AGENT_PROVIDER_CAPABILITY_UNSUPPORTED".into(),
                ))
            })
        }
    }

    /// 只提交 durable intent；原 Prompt owner 负责 exact session 通知及 Runtime 收敛。
    fn cancel<'a>(
        &'a self,
        context: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>> {
        #[cfg(any(windows, target_os = "macos"))]
        {
            Box::pin(async move {
                let failed = || ProviderError {
                    code: ProviderErrorCode::AgentProviderOperationFailed,
                };
                let row = self
                    .store
                    .execution(context.execution_id.clone())
                    .await
                    .map_err(|_| failed())?
                    .ok_or_else(failed)?;
                if row.provider != "codebuddy" {
                    return Err(failed());
                }
                self.store
                    .request_cancel(context.execution_id, crate::agent::coordinator::now())
                    .await
                    .map_err(|_| failed())?;
                Ok(())
            })
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = context;
            Box::pin(async { Err(unsupported()) })
        }
    }

    /// 恢复只依赖 durable ownership，与 CLI presence、enabled 和 admission health 无关。
    fn startup_reconcile<'a>(
        &'a self,
        _context: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        Box::pin(async {
            super::recovery::startup_with_launch(
                &self.store,
                &self.owner,
                self.resolved_launch_spec(),
            )
            .await
            .map_err(|_| ProviderError {
                code: ProviderErrorCode::AgentProviderOperationFailed,
            })
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, target_os = "macos"))]
#[path = "provider/macos_tests.rs"]
mod macos_tests;
