# Capability / descriptor snapshot

Exact typed snapshots are asserted in codebuddy/provider/tests.rs; exact public camelCase JSON snapshots are asserted in product/provider_catalog_tests.rs. No production conversion/serialization was added.

| Fact | Windows | non-Windows |
| --- | --- | --- |
| id / displayName | codebuddy / CodeBuddy | codebuddy / CodeBuddy |
| version when parsed product_version exists | product_version only | product_version only |
| version when absent | internal None; catalog omits version | internal None; catalog omits version |
| canExecute | false | false |
| canContinue | false | false |
| canCancel | false | false |
| canRecover | true | false |
| activity | false | false |
| tokenUsage | false | false |
| availableForNewExecution | false | false |

The non-Windows column describes cfg compile semantics, not executed non-Windows evidence. Each exact expectation uses cfg!(windows); the five unimplemented flags are explicit false. A Windows true literal is not shared across platforms. The existing Codex fixture has only cfg(windows) consumers and is unchanged.

Product test covers enabled=true and false, each with discovery missing -> found(product_version=2.158.0) -> found(no product_version) -> missing. base_version=1.106.1, package_version=0.0.0-deadbeef and build-deadbeef path must never appear as public version. Every step refreshes the actual adapter, retains the Codex instance and its health, asserts the entire CodeBuddy public entry and leaves durable execution/claim/runtime tables empty. This is a projection test, not a recovery rewrite or execution probe.

Registry get_registered works regardless of health and capability. get rejects unavailable; available get returns the registered adapter, but real ProviderAdmissionPolicy rejects Execute with AGENT_PROVIDER_CAPABILITY_UNSUPPORTED even when enabled. get is not an execution authorization API.

Recovery authority is additionally covered by the unchanged CB6-005 disabled_missing_cli_refresh_retains_recovery_authority test using a reopened durable store, disabled provider, missing discovery, refresh and startup reconcile. That test plus audited refresh self.store/self.owner construction supplies authority coverage without new recovery behavior.
