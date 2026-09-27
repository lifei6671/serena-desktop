# CB2-002 Provider Admission Policy

## Goal

Implement the frozen provider enabled admission gate and focused regressions only.

## Requirements

- Implement only task card CB2-002 from the versioned task breakdown.
- Resolve new provider work in the strict order registered, enabled, health, capability.
- Apply the enabled gate to start, continue, and resume pending.
- Return the stable `AGENT_PROVIDER_DISABLED` error for a registered disabled provider.
- Keep cancel and startup reconcile registration-only, including when disabled or unavailable.
- Preserve pending Execution and Workspace Claim when resume pending is disabled.
- Disabling must not cancel a running Execution, kill its Runtime, or release its Claim.
- Execution-local failure must not mutate global Provider health.
- Preserve Codex Runtime, Claim, Recovery, and Atomic Release behavior.

## Constraints

- No Remote MCP schema or fields.
- No UI.
- No CodeBuddy Runtime or ACP work.
- No CB2-003 Local IPC work.
- No Git commit.
- Stop with `DESIGN_BLOCKER` if implementation requires a new unfrozen authority, a Provider health state-machine change, weaker Claim/unknown fail-closed behavior, or broader scope.

## Acceptance Criteria

- [ ] Disabled start returns `AGENT_PROVIDER_DISABLED` before Provider execution.
- [ ] Disabled continue returns `AGENT_PROVIDER_DISABLED` without creating a continuation.
- [ ] Disabled resume pending preserves the Execution and Claim.
- [ ] Disabled cancel still invokes the persisted/registered Provider.
- [ ] Disabled startup reconcile still invokes recoverable registered Providers.
- [ ] Disabled and unavailable return distinct stable errors.
- [ ] A running Execution survives a later disable.
- [ ] Existing Codex happy path remains green.

## Source Contracts

- `docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md` CB2-002
- `docs/technical-design-multi-agent-provider-codebuddy-v0.1.md` sections 6 and 10
