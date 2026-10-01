# CB2-002 Contract Notes

- Verified task-breakdown SHA256: `7e74f8ac766daf1c0c0e5c3cdaa68fee1db666ff8085084b18c5a24ea91e02a1`.
- Verified technical-design SHA256: `f9f416b22a74b86f842349824009ef4a32da9e21054fb023387d0cde5d77a32b`.
- CB2-001 currently defines `AgentProviderSettings.providers` and defaults Codex enabled / CodeBuddy disabled.
- `ProviderRegistry::get` owns health admission; `get_registered` intentionally bypasses health for cancel and startup reconcile.
- `AgentTaskManager::dispatch_with_receipt` resolves health before Provider execution; continuation currently checks registration and capability, while resume shares dispatch.
- Product startup currently creates `AgentTaskManager` without provider settings, so persisted settings need focused startup wiring.
