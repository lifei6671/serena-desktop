# Provider catalog narrow regression fixes

## Goal

Repair five confirmed regressions introduced or exposed by the existing role/provider execution-default work without redesigning its contracts.

## Requirements

- Restore CodeBuddy generic `desired.option` ACK reconciliation so a returned `configOptions` entry with `category=mode` updates legacy `response.modes.current_mode_id`; preserve model-then-reasoning ordering.
- Separate AgentPanel catalog in-flight state from successful cache state. Failed requests must become retryable on later polling, each workspace/generation/provider key must remain single-flight, and successful keys must not restart a temporary Runtime every 1.5 seconds.
- Keep workspace/generation/provider catalog keys independent and never mutate saved `roleDefaults` during retry or transient unavailability.
- Resolve and freeze canonical workspace authority while holding `Broker.management`, then release that lock before awaiting Provider catalog I/O. Continue to reject frontend roots and retain TaskManager admission checks.
- At widths up to 980px, keep the label in the left grid column and stack Provider, Model, Reasoning, and status in the right column. Preserve the wide four-column layout.
- Replace the two obsolete opaque-profile tests with typed sparse `{model?, reasoning?}` canonicalization and unknown-field rejection coverage. Do not widen the production schema.
- Add focused Rust, frontend, and CSS contract regression tests for these behaviors.
- Preserve all pre-existing workspace changes and the committed `ef84516` and `54ec425`; do not commit or push.

## Acceptance Criteria

- [ ] The named CodeBuddy fresh regression test passes and proves legacy mode reconciliation remains compatible.
- [ ] AgentPanel proves first catalog rejection followed by same-key resolution restores selects while preserving saved defaults, without duplicate in-flight requests or post-success polling restarts.
- [ ] A blocked fake Provider catalog call does not block same-Broker role-route/default mutation.
- [ ] The narrow-grid CSS contract pins all three selects and status to the right column while the wide layout remains unchanged.
- [ ] ExecutionProfile tests enforce typed sparse canonicalization and deny unknown fields.
- [ ] All six user-requested validation commands complete with honestly reported results.
- [ ] Final delivery review covers only the delivery-owned hunks and reports any additional P0/P1.

## Notes

- Non-goals: Codex provider/app-server changes, production execution/store/profile schema changes, new Provider features, Trellis parent-task redesign, unrelated refactors, commits, and pushes.
