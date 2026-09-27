# CB2-002 Design

## Boundary

Add a provider control admission helper that combines the existing compile-time registry, the CB2-001 `AgentProviderSettings` policy, existing registry health, and provider capabilities. The helper is the single ordering owner for new provider work.

## Admission Flow

1. Resolve the provider with registration-only lookup.
2. Read the local enabled policy for that registered provider.
3. Reject disabled providers with `AgentProviderDisabled` / `AGENT_PROVIDER_DISABLED`.
4. Read existing registry health and reject unavailable providers with the existing unavailable error.
5. Check the capability required by the operation.

Start, continue, and resume pending use this flow. Cancel and startup reconcile keep using `get_registered` and their existing capability checks.

## State and Lifecycle Invariants

- `ManagerConfig.agent_providers` remains the Local Human Authority; the admission layer only consumes its settings.
- The registry health model remains `Available` / `Unavailable`.
- The gate runs before provider execution and does not mutate Execution, Runtime, Claim, or health.
- A running provider future keeps its resolved provider ownership even if later policy changes.
- Resume rejection occurs before dispatch-state mutation so the persisted Execution and Claim remain unchanged.

## Compatibility

Default settings keep Codex enabled, preserving existing constructors and tests. Product startup consumes persisted provider settings without adding Remote MCP fields or Local IPC mutation APIs.

## Verification

Focused Rust tests cover every frozen matrix row plus the existing Codex happy path. Run the project-prescribed focused test command and the relevant Rust compile/lint gate available in the repository.
