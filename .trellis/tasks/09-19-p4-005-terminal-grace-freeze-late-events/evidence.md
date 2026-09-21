# P4-005 Evidence

## Baseline

- Starting HEAD: recorded before implementation.
- Pre-existing P4-001～P4-004 dirty and untracked paths are excluded from this delivery unit unless a P4-005 hunk is added deliberately.

## Verification

- `cargo test --locked agent::store::usage_tests --lib`: PASS, 11 tests.
- `cargo test --locked agent::store::tests --lib`: PASS, 30 tests.
- `cargo test --locked agent::telemetry_projector --lib`: PASS, 3 tests.
- `cargo test --locked agent::codex::provider::adapter_tests --lib`: PASS, 28 tests.
- `cargo check --locked`: PASS; only pre-existing dead-code warnings.
- `cargo fmt --all -- --check`: PASS.
- `git diff --check`: PASS (line-ending warnings only).

Provider fake transport waits for persisted `completed` plus absent workspace Claim before emitting late Usage. It covers exact +1 Usage, wrong-turn drop without grace invalidation, Activity/Permission ignore, EOF-triggered freeze, paused-clock already-expired drain with a queued Usage that leaves public total/revision unchanged, and injected grace/freeze/projection failures that preserve the completed Execution result.

Store tests cover first terminal timestamp, duplicate grace idempotence, inclusive 2000ms acceptance, 2001ms atomic freeze/error, frozen no-op, public completeness/revision stability, and runtime-evidence same-transaction freeze isolation.

## Review

Independent read-only review identified two deadline P1 findings; both were repaired before final verification: deadline is captured before Store I/O, and drain has an immediate deadline precheck plus biased deadline selection. Final independent read-only re-review: PASS, P0=0 and P1=0.
