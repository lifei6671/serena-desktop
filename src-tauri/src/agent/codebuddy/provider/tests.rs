use super::*;
use crate::agent::provider::port::{AgentEventSink, ProviderAcceptanceSink};
use std::path::PathBuf;

/// lifecycle fail-closed 测试的 acceptance sentinel。
struct NoopAcceptance;

impl ProviderAcceptanceSink for NoopAcceptance {
    /// skeleton 测试不应接受执行，此方法仅满足 trait 输入。
    fn accepted(&self) {
        panic!("CodeBuddy skeleton must not accept execution")
    }
}

/// lifecycle fail-closed 测试的无状态 telemetry sink。
struct NoopTelemetry;

impl AgentEventSink for NoopTelemetry {}

/// missing CLI 仍注册 descriptor，但 Registry execution lookup 保持 unavailable。
#[test]
fn missing_cli_registers_unavailable_skeleton() {
    let mut registry = ProviderRegistry::new();
    register_codebuddy_provider_with_discovery(&mut registry, Err(DiscoveryError::not_found(true)))
        .unwrap();
    let id = ProviderId::new("codebuddy".into()).unwrap();
    let provider = registry.get_registered(&id).unwrap();
    assert_eq!(provider.descriptor().display_name, "CodeBuddy");
    assert_eq!(registry.health(&id).unwrap(), ProviderHealth::Unavailable);
    match registry.get(&id) {
        Err(error) => assert_eq!(error.code, ProviderErrorCode::AgentProviderUnavailable),
        Ok(_) => panic!("unavailable CodeBuddy resolved for execution"),
    }
    assert_eq!(
        provider.capabilities(),
        ProviderCapabilities {
            can_execute: false,
            can_continue: false,
            can_cancel: false,
            can_recover: false,
            activity: false,
            token_usage: false,
        }
    );
}

/// found CLI 只提升 Admission Health，不提前开放任何执行能力。
#[test]
fn found_cli_registers_available_capability_conservative_skeleton() {
    let mut registry = ProviderRegistry::new();
    register_codebuddy_provider_with_discovery(
        &mut registry,
        Ok(DiscoveryResult::direct_for_test(
            "C:/resolved/codebuddy.exe",
        )),
    )
    .unwrap();
    let id = ProviderId::new("codebuddy".into()).unwrap();
    assert_eq!(registry.health(&id).unwrap(), ProviderHealth::Available);
    let capabilities = registry.capabilities(&id).unwrap();
    assert!(!capabilities.can_execute);
    assert!(!capabilities.can_continue);
    assert!(!capabilities.can_cancel);
    assert!(!capabilities.can_recover);
    assert!(!capabilities.activity);
    assert!(!capabilities.token_usage);
}

/// resolved LaunchSpec 与 Release descriptor 分离，descriptor 不获得本机绝对路径。
#[test]
fn resolved_launch_spec_is_separate_from_default_descriptor() {
    let discovery = DiscoveryResult::direct_for_test("C:/resolved/node.exe");
    let provider = CodeBuddyProvider::from_discovery(Ok(discovery));
    assert_eq!(DEFAULT_LAUNCH_DESCRIPTOR.command, "codebuddy");
    assert_eq!(DEFAULT_LAUNCH_DESCRIPTOR.args, ["--acp"]);
    assert_eq!(
        provider.resolved_launch_spec().unwrap().executable,
        PathBuf::from("C:/resolved/node.exe")
    );
    assert_eq!(provider.descriptor().version, None);
    assert!(provider.discovery_provenance().is_some());
    assert!(provider.discovery_error().is_none());
}

/// base version 不能冒充 Provider 产品版本或 admission whitelist。
#[test]
fn descriptor_uses_only_product_version_metadata() {
    let mut discovery = DiscoveryResult::direct_for_test("C:/resolved/codebuddy.exe");
    discovery.metadata.base_version = Some("1.106.1".into());
    let provider = CodeBuddyProvider::from_discovery(Ok(discovery.clone()));
    assert_eq!(provider.descriptor().version, None);
    discovery.metadata.product_version = Some("2.158.0".into());
    let provider = CodeBuddyProvider::from_discovery(Ok(discovery));
    assert_eq!(provider.descriptor().version.as_deref(), Some("2.158.0"));
}

/// 即使绕过 admission 直接调用 trait，所有生命周期入口也必须 fail closed。
#[tokio::test]
async fn lifecycle_methods_never_start_unimplemented_behavior() {
    let provider = CodeBuddyProvider::from_discovery(Ok(DiscoveryResult::direct_for_test(
        "C:/resolved/codebuddy.exe",
    )));
    let execute = provider
        .execute(
            ProviderExecutionContext {
                execution_id: "execution".into(),
            },
            Arc::new(NoopAcceptance),
            Arc::new(NoopTelemetry),
        )
        .await;
    assert_eq!(
        execute,
        Err(ProviderExecutionFailure::State(
            "AGENT_PROVIDER_CAPABILITY_UNSUPPORTED".into()
        ))
    );
    assert_eq!(
        provider
            .cancel(ProviderCancelContext {
                execution_id: "execution".into(),
            })
            .await
            .unwrap_err()
            .code,
        ProviderErrorCode::AgentProviderCapabilityUnsupported
    );
    assert_eq!(
        provider
            .startup_reconcile(ProviderStartupContext {})
            .await
            .unwrap_err()
            .code,
        ProviderErrorCode::AgentProviderCapabilityUnsupported
    );
}
