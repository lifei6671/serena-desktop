use std::sync::{Arc, RwLock};

use crate::config::AgentProviderSettings;

use super::{
    ProviderError, ProviderErrorCode, ProviderId,
    port::AgentProvider,
    registry::{ProviderHealth, ProviderRegistry},
};

/// 新 Provider 工作可请求的能力；Cancel 与 startup reconcile 不经过此门禁。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderAdmissionCapability {
    Execute,
    Continue,
}

/// 消费本地 Human Authority 设置，并集中维护冻结的 admission 判定顺序。
#[derive(Clone)]
pub(crate) struct ProviderAdmissionPolicy {
    settings: Arc<RwLock<AgentProviderSettings>>,
}

impl From<AgentProviderSettings> for ProviderAdmissionPolicy {
    /// 独立 Manager 仍可从持久化快照建立策略。
    fn from(settings: AgentProviderSettings) -> Self {
        Self::new(settings)
    }
}
impl ProviderAdmissionPolicy {
    /// 落盘完成前持有写锁，admission 只能观察已提交策略。
    pub(crate) fn commit(
        &self,
        settings: AgentProviderSettings,
        persist: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let mut current = self
            .settings
            .write()
            .expect("provider admission policy lock poisoned");
        persist()?;
        *current = settings;
        Ok(())
    }

    /// 从已经加载并验证的 ManagerConfig 快照建立 admission policy。
    pub(crate) fn new(settings: AgentProviderSettings) -> Self {
        Self {
            settings: Arc::new(RwLock::new(settings)),
        }
    }

    /// 严格按 registered、enabled、health、capability 顺序解析新工作 Provider。
    pub(crate) fn admit(
        &self,
        registry: &ProviderRegistry,
        provider_id: &ProviderId,
        capability: ProviderAdmissionCapability,
    ) -> Result<Arc<dyn AgentProvider>, ProviderError> {
        let provider = self.registered_enabled(registry, provider_id)?;
        if registry.health(provider_id)? == ProviderHealth::Unavailable {
            return Err(ProviderError {
                code: ProviderErrorCode::AgentProviderUnavailable,
            });
        }
        let capabilities = provider.capabilities();
        let supported = match capability {
            ProviderAdmissionCapability::Execute => capabilities.can_execute,
            ProviderAdmissionCapability::Continue => capabilities.can_continue,
        };
        if !supported {
            return Err(ProviderError {
                code: ProviderErrorCode::AgentProviderCapabilityUnsupported,
            });
        }
        Ok(provider)
    }

    /// 在可能创建 Execution/Claim 的入口只完成不可变的 registered→enabled 前缀。
    pub(crate) fn registered_enabled(
        &self,
        registry: &ProviderRegistry,
        provider_id: &ProviderId,
    ) -> Result<Arc<dyn AgentProvider>, ProviderError> {
        let provider = registry.get_registered(provider_id)?;
        let enabled = self
            .settings
            .read()
            .map_err(|_| ProviderError {
                code: ProviderErrorCode::AgentProviderContractError,
            })?
            .providers
            .get(provider_id.as_str())
            .is_some_and(|policy| policy.enabled);
        if !enabled {
            return Err(ProviderError {
                code: ProviderErrorCode::AgentProviderDisabled,
            });
        }
        Ok(provider)
    }

    /// 测试专用策略切换，用于锁定 Drain 语义；生产 mutation surface 留给后续任务。
    #[cfg(test)]
    pub(crate) fn set_enabled_for_test(&self, provider_id: &str, enabled: bool) {
        self.settings
            .write()
            .expect("provider admission policy lock poisoned")
            .providers
            .entry(provider_id.to_owned())
            .or_insert(crate::config::AgentProviderPolicy { enabled })
            .enabled = enabled;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use crate::{
        agent::provider::{
            ProviderCapabilities, ProviderDescriptor, ProviderExecutionContext, ProviderRunResult,
            ProviderStartupContext,
            port::{
                AgentEventSink, ProviderAcceptanceSink, ProviderExecutionFailure, ProviderFuture,
                ProviderReconcileSummary,
            },
        },
        config::AgentProviderPolicy,
    };

    struct OrderedProvider {
        capability_calls: AtomicUsize,
        can_execute: bool,
    }

    impl OrderedProvider {
        /// 构造只记录 capability 读取次数的测试 Provider。
        fn new(can_execute: bool) -> Self {
            Self {
                capability_calls: AtomicUsize::new(0),
                can_execute,
            }
        }
    }

    impl AgentProvider for OrderedProvider {
        /// 返回固定身份，注册阶段不读取 capability。
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: ProviderId::new("ordered".into()).unwrap(),
                display_name: "Ordered".into(),
                version: None,
                protocol: None,
            }
        }

        /// 记录 admission 是否已经进入最后一个 capability 阶段。
        fn capabilities(&self) -> ProviderCapabilities {
            self.capability_calls.fetch_add(1, Ordering::SeqCst);
            ProviderCapabilities {
                can_execute: self.can_execute,
                can_continue: false,
                can_cancel: true,
                can_recover: true,
                activity: false,
                token_usage: false,
            }
        }

        /// 本测试只检查 admission，不允许实际执行。
        fn execute<'a>(
            &'a self,
            _context: ProviderExecutionContext,
            _acceptance: Arc<dyn ProviderAcceptanceSink>,
            _telemetry: Arc<dyn AgentEventSink>,
        ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>> {
            Box::pin(async { panic!("admission test must not execute provider") })
        }

        /// 本测试不进入 Cancel 路径。
        fn cancel<'a>(
            &'a self,
            _context: super::super::ProviderCancelContext,
        ) -> ProviderFuture<'a, Result<(), ProviderError>> {
            Box::pin(async { panic!("admission test must not cancel provider") })
        }

        /// 本测试不进入 startup reconcile 路径。
        fn startup_reconcile<'a>(
            &'a self,
            _context: ProviderStartupContext,
        ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
            Box::pin(async { panic!("admission test must not reconcile provider") })
        }
    }

    /// 构造只有 ordered Provider 条目的本地策略。
    fn settings(enabled: bool) -> AgentProviderSettings {
        let mut settings = AgentProviderSettings::default();
        settings
            .providers
            .insert("ordered".into(), AgentProviderPolicy { enabled });
        settings
    }

    /// 构造指定 health 的 Registry，并保留断言 capability 调用的 concrete handle。
    fn registry(provider: Arc<OrderedProvider>, health: ProviderHealth) -> ProviderRegistry {
        let mut registry = ProviderRegistry::new();
        registry.register(provider, health).unwrap();
        registry
    }

    /// 分层构造冲突事实，证明后层在前层拒绝时不会被读取。
    #[test]
    fn admission_order_is_registered_then_enabled_then_health_then_capability() {
        let missing_registry = ProviderRegistry::new();
        let policy = ProviderAdmissionPolicy::new(settings(false));
        let missing = policy
            .admit(
                &missing_registry,
                &ProviderId::new("ordered".into()).unwrap(),
                ProviderAdmissionCapability::Execute,
            )
            .err()
            .unwrap();
        assert_eq!(missing.code, ProviderErrorCode::AgentProviderNotFound);

        let disabled_provider = Arc::new(OrderedProvider::new(false));
        let disabled_registry = registry(disabled_provider.clone(), ProviderHealth::Unavailable);
        let disabled = policy
            .admit(
                &disabled_registry,
                &ProviderId::new("ordered".into()).unwrap(),
                ProviderAdmissionCapability::Execute,
            )
            .err()
            .unwrap();
        assert_eq!(disabled.code, ProviderErrorCode::AgentProviderDisabled);
        assert_eq!(disabled_provider.capability_calls.load(Ordering::SeqCst), 0);

        let unavailable_provider = Arc::new(OrderedProvider::new(false));
        let unavailable_registry =
            registry(unavailable_provider.clone(), ProviderHealth::Unavailable);
        let unavailable = ProviderAdmissionPolicy::new(settings(true))
            .admit(
                &unavailable_registry,
                &ProviderId::new("ordered".into()).unwrap(),
                ProviderAdmissionCapability::Execute,
            )
            .err()
            .unwrap();
        assert_eq!(
            unavailable.code,
            ProviderErrorCode::AgentProviderUnavailable
        );
        assert_eq!(
            unavailable_provider.capability_calls.load(Ordering::SeqCst),
            0
        );

        let unsupported_provider = Arc::new(OrderedProvider::new(false));
        let unsupported_registry =
            registry(unsupported_provider.clone(), ProviderHealth::Available);
        let unsupported = ProviderAdmissionPolicy::new(settings(true))
            .admit(
                &unsupported_registry,
                &ProviderId::new("ordered".into()).unwrap(),
                ProviderAdmissionCapability::Execute,
            )
            .err()
            .unwrap();
        assert_eq!(
            unsupported.code,
            ProviderErrorCode::AgentProviderCapabilityUnsupported
        );
        assert_eq!(
            unsupported_provider.capability_calls.load(Ordering::SeqCst),
            1
        );
    }
}
