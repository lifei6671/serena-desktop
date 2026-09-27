# CB4-002 Implementation Plan

1. Capture dirty baseline after CB4-001 and inspect AgentPanel catalog polling, types/api, existing provider local IPC tests.
2. Add typed frontend API wrapper for agent_provider_set_role_route (and settings type reuse). No new backend mutation endpoint unless strictly missing.
3. Add provider-neutral role editor presentation helpers/constants.
4. Render 角色分工 section between Provider cards and task Composer using shadcn Select.
5. Implement per-role optimistic/draft save with pending state and rollback on failure; protect against stale catalog polling overwriting an in-flight/newly committed value.
6. Preserve disabled and unknown current bindings; allow explicit clear.
7. Add DOM tests: set, clear, disabled, unknown, failure rollback, stale polling, reload/persisted fixture, no auto-rebind.
8. Reuse existing backend provider-policy tests; add focused test only if client contract reveals a gap.
9. Run frontend focused/full tests, build, lint; backend provider-policy + registry/hash regressions; cargo check/fmt/diff-check.
10. Freeze delivery and wait for Host Gate. Do not enter CB4-003.

## Rollback Point

If implementation requires Remote mutation, automatic fallback, or changes to Runtime/routing execution semantics, return DESIGN_BLOCKER.