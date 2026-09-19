# P4-004 Evidence

## Scope

- Authority: technical design §§32–33、36.2、38 and revision003 P4-004.
- Host-supplied SHA-256 values for the nine authority files matched before edits.
- Delivery-owned responsibility: Codex baseline intent/freeze and late-turn invalidation; Store epoch/private/public Usage projection; projector delegation; focused tests and this task record.
- Excluded: terminal grace/freeze (P4-005), Product, MCP, UI, account usage reads, response aggregation, commits, and push.

## Behavior assertions

- Fresh baseline is `{"totalTokens":0}` only with no current epoch row. Warm reads a valid current `(runtime,thread)` total; missing/corrupt/cold evidence is unknown.
- Public delta is only `incoming totalTokens - baseline totalTokens`; all five public breakdown columns are NULL. `modelContextWindow` is retained as metadata.
- Fresh `100 -> 150` produces `100 -> 150`; warm baseline `100`, then `150 -> 180`, produces `50 -> 80`.
- An identical public snapshot is a revision no-op; changed context window advances revision. Same-epoch `150 -> 140` returns `USAGE_COUNTER_REGRESSION` without changing epoch/private/public rows.
- A same runtime/thread old-turn event invalidates the matching baseline, does not publish, and degrades partial public Usage to unknown. Invalidation receives the event `observed_at`; an injected invalidation failure is diagnostic-only and the Execution still completes.

## Verification

| Command | Result |
| --- | --- |
| `cargo test --locked agent::store::usage_tests --lib` | PASS — 7 passed |
| `cargo test --locked agent::store::tests --lib` | PASS — 30 passed |
| `cargo test --locked agent::telemetry_projector::tests --lib` | PASS — 3 passed |
| `cargo test --locked agent::codex::provider::adapter_tests --lib` | PASS — 23 passed |
| `cargo check --locked` | PASS — 6 pre-existing dead-code warnings, no error |
| `cargo fmt --all -- --check` | PASS |
| `git diff --check` | PASS |

## Independent review

- Round 1 found a P1 timestamp defect and mandatory provider behavior-test gaps.
- Repair: invalidation uses `usage.observed_at`; real transport tests prove the freeze boundary, late-turn unknown degradation, and fault isolation.
- Round 2 independent read-only review: **PASS**, P0/P1 = 0. It verified the repaired event timestamp, the real transport/provider behavior tests, Store fault isolation, and the P4-005 scope fence.
