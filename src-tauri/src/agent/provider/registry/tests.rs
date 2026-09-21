use std::sync::Arc;

use super::*;
use crate::agent::provider::{
    ProviderCancelContext, ProviderExecutionContext, ProviderRunResult, ProviderStartupContext,
    port::{
        AgentEventSink, ProviderAcceptanceSink, ProviderExecutionFailure, ProviderFuture,
        ProviderReconcileSummary,
    },
};

struct FakeProvider {
    descriptor: ProviderDescriptor,
    capabilities: ProviderCapabilities,
}

impl FakeProvider {
    fn new(id: &str, display_name: &str, can_execute: bool) -> Self {
        Self {
            descriptor: ProviderDescriptor {
                id: ProviderId::new(id.into()).unwrap(),
                display_name: display_name.into(),
                version: None,
            },
            capabilities: ProviderCapabilities {
                can_execute,
                can_continue: false,
                can_cancel: false,
                can_recover: false,
                activity: false,
                token_usage: false,
            },
        }
    }
}

impl AgentProvider for FakeProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }

    fn execute<'a>(
        &'a self,
        _context: ProviderExecutionContext,
        _acceptance: Arc<dyn ProviderAcceptanceSink>,
        _telemetry: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>> {
        Box::pin(async { panic!("registry must not execute providers") })
    }

    fn cancel<'a>(
        &'a self,
        _context: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>> {
        Box::pin(async { panic!("registry must not cancel provider work") })
    }

    fn startup_reconcile<'a>(
        &'a self,
        _context: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        Box::pin(async { panic!("registry must not reconcile providers") })
    }
}

fn provider(id: &str, display_name: &str, can_execute: bool) -> Arc<dyn AgentProvider> {
    Arc::new(FakeProvider::new(id, display_name, can_execute))
}

fn provider_id(id: &str) -> ProviderId {
    ProviderId::new(id.into()).unwrap()
}

#[test]
fn register_and_get_return_the_same_available_provider() {
    let mut registry = ProviderRegistry::new();
    let registered = provider("available", "Available Provider", true);
    registry
        .register(registered.clone(), ProviderHealth::Available)
        .unwrap();

    let resolved = registry.get(&provider_id("available")).unwrap();
    assert!(Arc::ptr_eq(&registered, &resolved));
    assert!(Arc::ptr_eq(
        &registered,
        &registry.get_registered(&provider_id("available")).unwrap()
    ));
    assert_eq!(resolved.descriptor().display_name, "Available Provider");
}

#[test]
fn duplicate_id_returns_contract_error_without_replacing_the_original() {
    let mut registry = ProviderRegistry::new();
    let original = provider("duplicate", "Original Provider", true);
    let duplicate = provider("duplicate", "Replacement Provider", false);
    registry
        .register(original.clone(), ProviderHealth::Available)
        .unwrap();

    assert_eq!(
        registry.register(duplicate.clone(), ProviderHealth::Available),
        Err(ProviderError {
            code: ProviderErrorCode::AgentProviderContractError,
        })
    );

    let resolved = registry.get(&provider_id("duplicate")).unwrap();
    assert!(Arc::ptr_eq(&original, &resolved));
    assert!(!Arc::ptr_eq(&duplicate, &resolved));
    assert_eq!(resolved.descriptor().display_name, "Original Provider");
}

#[test]
fn unknown_id_returns_not_found_from_every_lookup() {
    let registry = ProviderRegistry::new();
    let unknown = provider_id("unknown");
    let expected = ProviderError {
        code: ProviderErrorCode::AgentProviderNotFound,
    };

    match registry.get(&unknown) {
        Err(error) => assert_eq!(error, expected),
        Ok(_) => panic!("unknown provider resolved for execution"),
    }
    match registry.get_registered(&unknown) {
        Err(error) => assert_eq!(error, expected),
        Ok(_) => panic!("unknown provider resolved from registration"),
    }
    assert_eq!(registry.capabilities(&unknown), Err(expected.clone()));
    assert_eq!(registry.health(&unknown), Err(expected));
}

#[test]
fn unavailable_provider_remains_discoverable_but_cannot_be_resolved_for_execution() {
    let mut registry = ProviderRegistry::new();
    let registered = provider("unavailable", "Unavailable Provider", true);
    registry
        .register(registered.clone(), ProviderHealth::Unavailable)
        .unwrap();
    let id = provider_id("unavailable");

    assert_eq!(registry.health(&id).unwrap(), ProviderHealth::Unavailable);
    assert_eq!(
        registry
            .list_descriptors()
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect::<Vec<_>>(),
        vec![id.clone()]
    );
    assert!(registry.capabilities(&id).unwrap().can_execute);
    match registry.get(&id) {
        Err(error) => assert_eq!(error.code, ProviderErrorCode::AgentProviderUnavailable),
        Ok(_) => panic!("unavailable provider resolved for execution"),
    }
    assert!(Arc::ptr_eq(
        &registered,
        &registry.get_registered(&id).unwrap()
    ));
    assert_eq!(registry.health(&id).unwrap(), ProviderHealth::Unavailable);
}

#[test]
fn health_update_changes_only_health_for_a_registered_provider() {
    let mut registry = ProviderRegistry::new();
    let registered = provider("mutable-health", "Mutable Health Provider", true);
    let id = provider_id("mutable-health");
    let descriptor = registered.descriptor();
    let capabilities = registered.capabilities();
    registry
        .register(registered.clone(), ProviderHealth::Available)
        .unwrap();

    registry
        .set_health(&id, ProviderHealth::Unavailable)
        .unwrap();
    assert_eq!(registry.health(&id).unwrap(), ProviderHealth::Unavailable);
    match registry.get(&id) {
        Err(error) => assert_eq!(error.code, ProviderErrorCode::AgentProviderUnavailable),
        Ok(_) => panic!("health-gated lookup returned an unavailable provider"),
    }
    assert!(Arc::ptr_eq(
        &registered,
        &registry.get_registered(&id).unwrap()
    ));
    assert_eq!(registry.list_descriptors(), vec![descriptor.clone()]);
    assert_eq!(registry.capabilities(&id).unwrap(), capabilities);

    registry.set_health(&id, ProviderHealth::Available).unwrap();
    assert_eq!(registry.health(&id).unwrap(), ProviderHealth::Available);
    assert!(Arc::ptr_eq(&registered, &registry.get(&id).unwrap()));
    assert_eq!(registry.list_descriptors(), vec![descriptor]);
    assert_eq!(registry.capabilities(&id).unwrap(), capabilities);
}

#[test]
fn health_update_rejects_an_unknown_provider() {
    let mut registry = ProviderRegistry::new();

    assert_eq!(
        registry.set_health(&provider_id("unknown"), ProviderHealth::Unavailable),
        Err(ProviderError {
            code: ProviderErrorCode::AgentProviderNotFound,
        })
    );
}

#[test]
fn list_descriptors_is_sorted_by_provider_id() {
    let mut registry = ProviderRegistry::new();
    registry
        .register(
            provider("zeta", "Zeta Provider", true),
            ProviderHealth::Unavailable,
        )
        .unwrap();
    registry
        .register(
            provider("alpha", "Alpha Provider", true),
            ProviderHealth::Available,
        )
        .unwrap();

    assert_eq!(
        registry
            .list_descriptors()
            .into_iter()
            .map(|descriptor| descriptor.id.as_str().to_owned())
            .collect::<Vec<_>>(),
        vec!["alpha".to_owned(), "zeta".to_owned()]
    );
}

#[test]
fn production_registry_has_no_provider_private_or_runtime_safety_branches() {
    let source = include_str!("../registry.rs");
    let identifiers: Vec<_> = source
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .collect();

    for forbidden in [
        "Codex",
        "codex",
        "Runtime",
        "runtime",
        "Claim",
        "claim",
        "thread",
        "turn",
        "job",
        "historyMode",
        "history_mode",
    ] {
        assert!(
            !identifiers.contains(&forbidden),
            "production registry contains forbidden text {forbidden}"
        );
    }
}
