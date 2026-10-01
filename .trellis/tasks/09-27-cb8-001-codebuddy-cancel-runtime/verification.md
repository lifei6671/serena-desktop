# CB8-001 verification and scope

Baseline: `feat/codebuddy`, `9316bb85dc50de873843d6d73cfce95b7229b1a4`, clean. No staged changes, commits or pushes. All validation is native Windows, from repository root. `C:/Users/lifei/.cargo/bin` and `C:/nvm4w/nodejs` were added only to command-local PATH. Exact final commands/exits/log names: `validation-results.jsonl`.

## Required checks

| Filter / command | Evidence |
| --- | --- |
| `codebuddy --lib -- --test-threads=1` | 134 PASS, exit 0 (`repair1-verified-codebuddy.log`); pre-review final 132 PASS |
| `agent::store::transactions::tests` | 54 PASS, exit 0 |
| `agent::codex::provider::cancellation_tests` | 15 PASS / 1 ignored real CLI smoke, exit 0 |
| `agent::task_manager::tests` | 31 PASS, exit 0 |
| `agent::provider::control` | 1 PASS, exit 0 |
| `agent::product::provider_catalog_tests` | 5 PASS, exit 0 |
| `mcp::orchestration_tests::provider_query_tests` | 4 PASS, exit 0 |
| `telemetry` / `same_runtime` / `usage` | 11 / 2 / 57 PASS, exit 0 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | PASS, exit 0 |
| `cargo check --manifest-path src-tauri/Cargo.toml --lib` | PASS, exit 0 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --lib --tests -- -D warnings` | FAIL exit 101: only unchanged `usage_tests.rs:987 await_holding_lock` (awaits 1012/1016), explicitly frozen baseline |
| `git diff --check` | PASS |
| Linux build/test | UNAVAILABLE: no project Docker runner found; no Linux validation attempted, no WSL |

## Requirements mapped to tests

- Generic pristine Codex local cancel/release versus bound Runtime or reserved attempt durable intent/retained Claim: `request_cancel_distinguishes_pristine_from_runtime_attempt`.
- Dispatching/Dispatched/Running idempotence, stable timestamp, ACK retains Claim, terminal unchanged: `request_cancel_inflight_and_terminal_preservation`, existing state/transaction and Codex cancellation regressions.
- Official SDK exact notification wire, single slot, duplicate/wrong session/no permit/other method/private extra fields, physical flush: `cancel_sdk_exact_wire_once`, `cancel_guard_permit_and_physical_flush`.
- Write/flush error is not ACK; blocked pipe timeout and cleanup: `write_and_flush_closed_pipe_keep_first_failure_and_health_local`, `cancel_blocked_pipe_times_out_and_clears_permit`.
- Exact response already read before owner wakes suppresses late wire: `cancel_guard_suppresses_wire_after_exact_prompt_response`.
- Public control with no Runtime, or R1 prepared before prompt, no fabricated Cancelled and evidence failure retains Claim: `native_cancel_before_prompt_retains_evidence_authority`, `codebuddy_cancel_history_ignores_disabled_unavailable_health`.
- Acceptance before physical prompt flush (including failed prompt flush), durable intent first: `native_cancel_accepted_before_prompt_flush` uses explicit synchronization.
- Cancel after prompt flush with zero Workspace delta, after actual fixed marker write with retained delta, duplicate cancellation once, exact cancelled or actual end_turn, registered unavailable/disabled historical control: `native_cancel_exact_terminal_and_workspace_matrix`.
- Natural terminal before cancel preserves Completed and zero cancel wire: additional assertion in `native_staged_live_job_startup_preserves_terminal_and_result`.
- No terminal bounded timeout / real pipe send failure / staged Cancelled with live Job and retained Claim: `native_cancel_timeout_pipe_failure_and_staged_claim`.
- Exact Cancelled plus failed durable evidence -> Unknown/Claim, startup preserves terminal and releases only with approved evidence: `native_cancel_terminal_evidence_failure_and_startup`.
- Startup after persisted intent / physical cancel without terminal -> Interrupted only: `native_cancel_startup_after_intent_or_send`.
- Direct wrong-provider execution rejected; unavailable registered adapter persists intent without CLI: `cancel_unavailable_registered_history_and_wrong_provider`.
- Existing Fresh, Activity, Recovery, caller drop, competing owner, provider-control, telemetry, same-runtime and Usage tests remain in the required filters.

## Preserved failures and repairs

- `client-initial.log`: existing client tests 26 PASS.
- `cancel-first.log`: new test compile error (use after consuming client shutdown), fixed by retaining slot handle.
- `cancel-second.log`: 76 PASS / 1 ignored, exit 0.
- `codebuddy-first.log`: core CodeBuddy module parallel 111 PASS / 2 FAIL: OCC fixture cleanup timeout, preflush fixture observed dispatched rather than uncertain. Both retained; deterministic serial `codebuddy-serial.log` 114 PASS and `codebuddy-expanded-serial.log` 116 PASS, then broader `codebuddy-gate.log` 131 PASS. Do not label parallel run passed.
- `final-codebuddy.log`: compile failure from unqualified test `now()`, fixed to coordinator path. No test bodies counted.
- `verified-codebuddy.log`: 131 PASS / 1 FAIL from obsolete canCancel=false assertion; changed only assertion to cfg!(windows), then reran full CodeBuddy filter.

No real CodeBuddy/CB5 probes. Ignored real Codex smoke is not runtime evidence. Clippy baseline is not suppressed or repaired. CB5 contract/design, Codex source, Usage source, schema, dependencies and lockfile remain unchanged. No Continue, permission, registry, retry or new Runtime on cancel. Runtime termination authority remains whole Job + durable evidence; no PID proof.

## Review

Final strategy: independent CHILD_AGENT, read-only FULL_SCOPE, all delivery-owned code/tests/catalog plus task contracts. `freeze.json` records exact per-file hashes and aggregate target. Task status/review reports/verification logs are review context, not executable target. Final verdict APPROVED / gate PASSED; coverage COMPLETE; freshness FRESH (22/22 hashes); P0/P1/P2 all zero after one repair round. See `review-final.md`. Windows canCancel is enabled after the implementation gates and frozen CB5-004 contract; Clippy baseline and Linux unavailability remain explicitly separate from passing native implementation gates.

## Independent review repair round 1

Round 0 found one P1 (see `review-round0.md` and preserved `freeze-round0.json`): first cancel could invalidate the generic revision captured before terminal persistence. Fixed with an exact-response-only atomic Store path combining optional provider request ID + terminal, validating full expected private snapshot/revision and current original Runtime/live generic lifecycle in the transaction. Ordinary OCC APIs are unchanged; no terminal retry or Claim release is added.

`cancel_between_exact_response_and_atomic_terminal_preserves_result` uses task-local deterministic handshakes after response receipt and before the terminal transaction. Both absent/present provider request ID preserve Completed/text, no cancel wire, retained Claim until Job evidence. `atomic_prompt_response_cancel_and_conflict_matrix` checks cancel tolerance, stale private/session/Runtime, existing private/generic terminal, reconciling, and SQL failure rollback. Existing OCC conflicting private terminal tests still pass.

Repair verification: targeted race 1 PASS; complete CodeBuddy 134 PASS (`repair1-verified-codebuddy.log`, exit 0); fmt/check PASS; clippy still exactly the unchanged usage_tests.rs:987 baseline. Generic request_cancel and Codex/TaskManager/MCP/Usage code were not changed by repair, so previously passed focused regressions remain applicable.

Preserved repair test failure `repair1-full-codebuddy.log`: 132 PASS / 2 FAIL. Updated old private revision count from +3 to +2 because request identity/terminal now commit together; changed wrong-Runtime test injection to stale caller snapshot instead of attempting a database rebind forbidden by existing trigger. No safety trigger or valid behavior test weakened.
