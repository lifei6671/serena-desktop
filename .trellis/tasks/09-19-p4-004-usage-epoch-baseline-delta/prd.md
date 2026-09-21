# P4-004 Usage Epoch Baseline、Delta 与 Continue

## Goal

Implement Codex epoch-scoped Usage baseline, total-only delta, Continue degradation, and late-turn pollution invalidation without terminal grace, Product, MCP, or UI changes.

## Requirements

- `CodexUsageEpoch` is exactly `(provider_id='codex', runtime_instance_id, thread_id)`; only same-epoch provider-supplied cumulative `totalTokens` may be compared or subtracted.
- Freeze a baseline after successful Thread bind and before `turn_start_observed`: fresh creates a provenance-backed zero only when the epoch has no row; warm reuse reads a valid same-epoch observed total; cold/resume and insufficient proof are unknown.
- Store baseline preparation, private epoch/state persistence, public total-only delta projection, duplicate revision semantics, same-epoch regression rejection, and public unknown degradation. Public breakdown fields remain NULL and `modelContextWindow` is metadata only.
- Synchronize a newly bound turn identity into pre-existing private Usage state in the same `bind_protocol_identity` transaction without allowing private mismatch to affect Execution lifecycle.
- Detect a late Usage notification for another turn of the same root/runtime, invalidate this execution's matching private baseline, and degrade an existing partial public Usage to unknown. All baseline/projection/invalidation failures are observability-only.
- Do not implement terminal grace/freeze, Account usage reads, Product/MCP/UI, response aggregation, or cross-runtime subtraction. Do not commit or push.

## Acceptance Criteria

- [ ] Baseline tests prove fresh-zero provenance, same-epoch warm observation, missing/corrupt/cold unknown fallback, and rejection after a turn has bound.
- [ ] Projection tests prove total-only fresh/warm deltas, unknown fallback, regression atomicity, cross-runtime isolation, duplicate no-op revision, context metadata revision, NULL/zero preservation, and absent-state fail-safe behavior.
- [ ] Provider/projector tests prove the freeze boundary and intent selection, late-turn pollution degradation, error isolation, and projector private-identity opacity.
- [ ] Focused Store/projector/provider tests, `cargo test --locked agent::store::tests --lib`, `cargo check --locked`, formatter check, and diff check have recorded actual outcomes.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
