# Role Provider Default Execution Configuration

## Goal

Extend Agent role routing into role-by-provider default execution configuration. Each role/provider pair independently stores an optional model and reasoning choice. The selected values are resolved and frozen only when a new Execution is created; retry identity includes the frozen profile, while Continue and recovery inherit the source profile.

## Requirements

- Preserve `AgentProviderSettings.roleRouting` and add a serde-defaulted sparse `roleDefaults` map keyed only by the five task roles and then by any valid Provider ID.
- Model and reasoning values are provider-neutral optional non-empty bounded strings. Missing or `null` means follow the Provider default; no Product/UI contract exposes provider-specific option names.
- Add a read-only, bounded execution-configuration catalog query that requires `providerId` and `workspaceId`, resolves the canonical workspace root from the registry, creates no Execution or Claim, and cleans up any temporary Provider runtime.
- CodeBuddy catalog discovery uses its guarded managed ACP lifecycle and derives models and reasoning values from live `session/new` data. Start applies model first and reasoning second after the model acknowledgement/update; unset fields send no override.
- Codex compatibility requires `model/list` plus the generated request capabilities used by `thread/start` and `turn/start`. Catalog discovery paginates `model/list`; model-specific reasoning options/defaults remain authoritative. Start and Continue send the frozen profile without overriding unset defaults.
- Replace the hard-coded `{}` profile assumption with a typed sparse provider-neutral ExecutionProfile. Start resolves defaults inside Broker management authority. Retry compares the current resolved profile. Continue and crash recovery validate and inherit the source profile verbatim.
- Add a management-locked local mutation for one role/provider default pair. Preserve valid settings for disabled or unregistered Providers and define/test null/null normalization without losing UI semantics.
- Extend Product/MCP provider snapshots with `roleDefaults` without causing catalog runtime startup. MCP callers cannot supply model/reasoning overrides.
- Update AgentPanel role rows to show role, Provider, default model, and reasoning using project Select controls. Preserve saved unavailable values, model-specific reasoning choices, pending/error behavior, and no-workspace/unavailable states.
- Preserve the two existing Host commits, do not rewrite them, and do not commit or push this task's changes.

## Acceptance Criteria

- [ ] Old configuration without `roleDefaults` loads unchanged; validation rejects invalid role keys, Provider IDs, and empty/overlong option strings while retaining valid unknown Providers.
- [ ] Switching a role between Providers restores each pair's persisted model/reasoning values after reload.
- [ ] New Start persists a canonical sparse profile; same request key with changed defaults conflicts; Continue and recovery inherit and validate the source profile despite current setting changes.
- [ ] CodeBuddy catalog parsing and sequential model-then-reasoning application are covered, including fail-closed invalid saved values and complete temporary-runtime cleanup.
- [ ] Codex catalog parsing/pagination, hidden-model handling, model-specific reasoning, and thread/turn wire parameters are covered.
- [ ] Catalog query creates no Execution/Claim and reports stable unknown/disabled/unavailable errors without deleting saved defaults.
- [ ] AgentPanel tests cover four-column role rows, Provider memory, unavailable saved values, model-dependent reasoning, and missing-workspace/unavailable states.
- [ ] Existing routing and provider fixtures remain compatible and are updated only where the additive field is observable.
- [ ] Relevant Rust and frontend tests pass, plus `cargo check`, `cargo clippy`, `cargo fmt --check`, and `git diff --check`; any pre-existing/environment failure is classified exactly.
- [ ] Final frozen diff receives complete independent review coverage with no remaining P0/P1 finding.

## Notes

- Scope is limited to role/provider execution defaults, provider-owned catalogs/application, frozen profile propagation, the local API/Product projection, AgentPanel, and directly related tests/fixtures.
- No migration, public caller override, shell fallback, global timeout change, unrelated cleanup, commit, or push is authorized.
