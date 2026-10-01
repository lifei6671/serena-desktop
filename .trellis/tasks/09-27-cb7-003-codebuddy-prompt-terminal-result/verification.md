# Verification (in progress)
Baseline 8499fef2378fdf18ec61ef6a472b2bc485491ed5, clean feat/codebuddy. Native Windows only. Linux UNAVAILABLE: no project Docker runner found in git tracked-file scan. No WSL invocation, no real CodeBuddy probe.

## Independently reproduced baseline failures
Isolated source archive of exact HEAD at C:/Users/lifei/AppData/Local/Temp/serena-cb7-003-baseline-8499fef/src-tauri. Commands use C:/Users/lifei/.cargo/bin/cargo.exe and PATH prepended C:/Users/lifei/.cargo/bin. Shared target cache E:/wx_lifeilin/github.com/lifei6671/serena-desktop/src-tauri/target; no working tree reset.
- `cargo test --lib --target-dir E:/wx_lifeilin/github.com/lifei6671/serena-desktop/src-tauri/target authority_state_failure_uses_stable_internal_code -- --nocapture`: FAIL exit101, 0 passed/1 failed. recovery/tests.rs:207 unwrap_err got Ok([]).
- `cargo test --lib --target-dir E:/wx_lifeilin/github.com/lifei6671/serena-desktop/src-tauri/target trait_startup_reconcile_projects_shared_recovery_errors_to_operation_failed -- --nocapture`: FAIL exit101, 0 passed/1 failed. adapter_tests.rs:1592 unwrap_err got empty ProviderReconcileSummary.
Both fixtures create orphan Claims; existing provider-scoped selection excludes nonexistent Execution. They remain recorded failures, never suppressed or counted as passing. Logs baseline-task-manager.log and baseline-codex.log; machine summary baseline-regressions.json.

Cleanup limitation: automatic execution policy rejected Remove-Item cleanup (blocked by policy) for task-created TEMP/serena-cb7-003-baseline-8499fef directory and matching .zip after tests. No retry/workaround. Temporary source copy remains; workspace was never reverted.

## Final native Windows verification
Cargo cwd: E:/wx_lifeilin/github.com/lifei6671/serena-desktop/src-tauri. Executable: C:/Users/lifei/.cargo/bin/cargo.exe. Exact commands/exits: validation-results.jsonl.

| Command arguments | Status | Counts / evidence |
|---|---|---|
| test --lib agent::codebuddy::prompt -- --nocapture | PASS exit0 | 8 passed, 0 failed/ignored; implement-prompt-tests.log |
| test --lib codebuddy -- --nocapture | PASS exit0 | 98 passed, 0 failed/ignored; codebuddy.log |
| test --lib agent::task_manager -- --nocapture | FAIL exit101, baseline reproduced | 47 passed, 1 failed, 1 ignored; task-manager.log |
| test --lib agent::codex -- --nocapture | FAIL exit101, baseline reproduced | 149 passed, 1 failed, 6 ignored; codex.log |
| fmt --all -- --check | PASS exit0 | fmt.log |
| check --lib --tests | PASS exit0 | check.log |
| clippy --lib --tests -- -D warnings | FAIL exit101, frozen baseline only | clippy.log |

Clippy only diagnostic: src/agent/store/usage_tests.rs:987:9 await_holding_lock; awaits 1012:46 and 1016:55. SHA256 C90595FCE0E1D57460648E3E28B2DC33A2C6E6AE7656C39C90AE96980958A778, matching baseline.json. No suppression or change. TaskManager and Codex failures exactly match independently reproduced baseline above. Counts overlap across focused/full filters and are not summed.
CodeBuddy suite includes CB7-002 fresh-session, CB6-005 recovery, private state and exact router regressions. Fake peer runs actual SDK, SQLite and Windows Job; not a real CodeBuddy/CB5 probe. Public provider execute/capabilities unchanged.

## Requirement trace
- Identity/send/accept ordering, single SDK wire, result privacy, terminal Runtime retention, late freeze: prompt::tests::exact_wire_order_terminal_result_and_late_freeze.
- Continuous bounded consumption beyond route TTL and result byte cap: continuously_drains_bounded_collector.
- EOF/remote/timeout/missing/wrong/malformed terminal meta/unknown stop/caller drop: failures_and_future_drop_are_uncertain_without_retry.
- Sanitized CB5 typed fields, five stop reasons and clean/tainted empty/nonempty completeness matrix: host_fixtures_and_all_typed_stop_mappings.
- Missing/malformed/foreign chunk correlation and malformed content: malformed_chunks_taint_but_foreign_identity_does_not.
- Permission denial alone waits without terminal; typed response persists exact stop reason: permission_is_not_terminal_and_typed_responses_persist.
- Wrong catalog/private session/R1/protocol, stale private revision, oversized prompt, previously accepted: rejected_identity_preflight_and_stale_revision_send_nothing.
- Concurrent private change or existing terminal cannot overwrite terminal; Sent conflict converges Uncertain: occ_conflicts_fail_closed_without_terminal_overwrite.

## Scope
Only CodeBuddy internal modules, prompt fake peer/Host excerpts and task evidence changed. No provider implementation/capability, generic finalization, Claim release, public Activity, Usage, cancel/load, schema, dependencies or canonical design edits. All source has Chinese documentation/comments. Independent FULL_SCOPE review pending freeze.

