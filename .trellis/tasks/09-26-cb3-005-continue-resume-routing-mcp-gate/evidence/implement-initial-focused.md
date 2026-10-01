# Initial targeted verification

cwd: repository root (Windows PowerShell)

Command: `cargo test --manifest-path src-tauri/Cargo.toml --locked continuation_routing_tests -- --nocapture`

Result: FAIL, exit 1; 2 passed / 4 failed / 0 ignored / 1279 filtered out.
Original output was observed in the tool transcript; this file records its material errors, not a replacement raw log.

- Three pending-based cases failed at the fixture assertion: actual status `dispatch_pending`, expected `pending`. Corrected the assertion to the real domain state.
- `continue_inherits_frozen_identity_while_query_reports_current_route` failed with `AGENT_LINEAGE_CONFLICT`, `requestAccepted=false`, `providerInvoked=false`, `dispatchCertainty=not_dispatched`.
- Root cause: `agent/store/transactions.rs` snapshot predicate still used `provider != 'codex'`, rejecting a legitimate frozen non-Codex continuation.
- Fix: compare against the canonical request's frozen Provider parameter. The integration test also verifies a forged different-Provider child fails `AGENT_SNAPSHOT_CONFLICT` without changing persisted evidence.
- Schema/parser and Continue rejection matrix already passed on this first run.
