# CB4-003 Implementation Plan

1. Capture post-CB4-002 baseline and inspect current AgentPanel catalog/role mutation generation logic.
2. Add typed frontend wrapper for existing agent_provider_set_enabled; no backend mutation endpoint changes.
3. Extend provider presentation input with optional diagnosticCode consumer seam only; do not change Remote Rust Catalog schema solely for fixture data.
4. Factor provider policy mutation state/generation so enabled mutation and role routing survive stale catalog polls independently.
5. Add Provider Card enable/disable control.
6. Detect loaded pending blockers from existing ExecutionView projection; render warning/actions without re-deriving Claim state.
7. Wire 查看任务 -> openDetails, 取消任务 -> existing cancel, 重新启用 -> local enabled IPC; no auto resume/unlock.
8. Add unsupported-version notice driven only by exact stable diagnosticCode.
9. Add DOM tests for pending claim, running draining, cancel/view/reenable, no auto-resume, disabled role, unsupported version, generic unavailable, no override/Force Unlock, mutation failure/stale poll.
10. Run focused/full frontend tests, build/lint, provider-policy and registry/hash regressions, CB3/4 routing focused gate, cargo check/fmt/diff-check.
11. Freeze delivery for Host screenshot/DOM review. Do not enter Phase 5.

## Rollback Point

If unsupported-version UX requires guessing from providerId/error text or changing admission/runtime semantics before Phase 5/6, do not do so. Preserve the UI seam and report the production-signal limitation explicitly.