# P4-003 Evidence

## Scope and authority

- Authority: technical design §§23, 31.0–31.1, 32.0 and the revision003 P4-003 task card.
- Pinned source SHA-256 values supplied by Host were rechecked before edits and all seven matched.
- Delivery-owned files: `agent/codex/protocol.rs`, `agent/codex/provider.rs`, `agent/codex/provider/adapter_tests.rs`, `agent/provider/telemetry.rs`, two direct telemetry API consumer test files, and this single P4-003 Trellis task.
- Excluded pre-existing work: P4-001, P4-002/schema_v9/store, Product, MCP and UI files.

## Results

| Check | Result | Evidence |
| --- | --- | --- |
| `cargo test --locked agent::codex::protocol --lib` | PASS | 11 passed, 0 failed, 1077 filtered |
| `cargo test --locked agent::codex::provider::adapter_tests --lib` | PASS | 19 passed, 0 failed, 1069 filtered |
| `cargo test --locked agent::provider::telemetry --lib` | PASS | 3 passed, 0 failed, 1085 filtered |
| `cargo test --locked agent::provider::port::tests::telemetry_contract_excludes_private_identity_evidence_and_wire_derives --lib` | PASS | 1 passed, 0 failed, 1086 filtered |
| `cargo test --locked agent::telemetry_projector::tests --lib` | PASS | 3 passed, 0 failed, 1084 filtered |
| `cargo check --locked` | PASS | completed; six pre-existing dead-code warnings, no error |
| `cargo fmt --all -- --check` | PASS | run from `src-tauri` |
| `git diff --check` | PASS | no diff error |

## Contract observations

- Recognized malformed Usage notifications return `USAGE_EVENT_INVALID`; unrelated notifications remain `Other`.
- `last` is strictly parsed but stops at the Codex-private struct. Safe event has no Thread, Turn, runtime or raw payload fields.
- Identity failure drops the telemetry hint without changing execution state. No Usage Store write, delta, baseline, regression filter, lifecycle or terminal behavior was introduced.
- P0-008 raw JSONL source file was later removed by cleanup and Git history has no committed raw/tls-final file. Parser regression therefore uses an evidence-derived accepted contract fixture: `threadId`/`turnId`/`tokenUsage.total`/`tokenUsage.last`/`modelContextWindow=258400`; test token values are explicitly synthetic and do not claim raw provenance.
- `total` and `last` now both require `totalTokens`、`inputTokens`、`cachedInputTokens`、`outputTokens` and `reasoningOutputTokens`; only `cacheWriteInputTokens` may be absent. Both snapshots reject missing/null required fields and explicit-null cache-write as `USAGE_EVENT_INVALID`; absent cache-write remains `None`, explicit zero remains `Some(0)`.

## DCR conflict

None found. The implementation keeps `totalTokens` provider-supplied, preserves absent cache-write as `None`, excludes `last` from accounting, and leaves P4-004 ownership untouched.

## Delivery review

- Independent read-only review, round 2: sink receipt coverage was repaired. Exact `100`, duplicate `100`, and out-of-order `50` are recorded in order by `RecordingEventSink` after the same strict mapper used by the notification loop.
- Gate: **PASS**. Host waived only direct original-JSONL provenance for this P4-003 regression fixture because P0-008 already accepted the observed schema contract. The waiver does not relax any field/schema, numeric, identity or publish behavior.
