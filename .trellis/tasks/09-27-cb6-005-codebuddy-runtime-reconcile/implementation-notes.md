# CB6-005 Implementation Notes

## Scope

CodeBuddy owns sealed Job evidence, Win32 observer, typed Runtime persistence, scoped startup reconciliation, and injected Store/Host authority. Discovery still controls only admission health. Codex Windows/macOS startup changes are only provider-scoped Claim/orphan selection; generic full-scope Store methods retain existing semantics. Schema v13, usage, Codex runtime evidence, result recovery, execute/continue/cancel remain untouched.

The existing generic `finalize_and_release_execution` already supports provider-neutral RuntimeTerminated convergence without Codex result recovery. CodeBuddy calls it with interrupted/unknown completeness/no result, so no new shared result abstraction or fake Session is necessary.

Single-execution OS/identity/private/evidence failure maps to existing summary kinds, preserves Claim, and attempts Runtime unknown. Generic existing Inconsistent/PendingExplicitResume/Released outcomes are preserved. Global Store failures continue to be Provider Err and retain TaskManager health contract.

## Verification in progress

Cwd: `E:\wx_lifeilin\github.com\lifei6671\serena-desktop`
Cargo: `C:/Users/lifei/.cargo/bin/cargo.exe`; command environment adds its directory to PATH for native rustc fixture compilation.

- Initial `cargo check --manifest-path src-tauri/Cargo.toml --lib --tests`: FAIL (WIP missing test module / Win32 import paths, then fixture uuid reference); repaired without dependencies.
- Initial `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::codebuddy::recovery::tests -- --nocapture`: FAIL, 4 passed / 4 failed; failures were test fixture CHECK, immutable runtime rebinding, and launcher frozen --acp argv. Repaired fixture inputs, did not weaken production constraints.
- Second focused recovery run (skip final capability test): FAIL, 11 passed / 1 failed; remaining malformed persisted bool fixture violated DB CHECK. Test-only corruption connection now explicitly disables CHECK enforcement; production Store retains schema constraints.
- Final focused results will be appended after Gate.

Final fmt/check/clippy, Codex regression, frozen-file hashes, scope and independent read-only full review are coordinated by main session. Existing baseline Clippy blocker is recorded separately in `baseline-clippy.log` (`usage_tests.rs:987 await_holding_lock`). No Linux/macOS runtime claim is made.

## Necessary generic missing-Runtime repair

A actual dangling Runtime FK fixture showed that generic `transition_execution` always rewrote unchanged `runtime_instance_id`, causing SQLite FK failure before `MarkUnknown` could persist. User explicitly requires missing original Runtime → generic unknown + Claim retained. The minimal repair writes binding only when changed, inside the same IMMEDIATE transaction; existing status/revision/dispatch CAS remains unchanged. No Provider ID branch or release predicate was added. Tests: `missing_original_runtime_can_be_marked_unknown_without_rebinding`, `dispatch_atomically_persists_changed_runtime_binding` plus complete existing transaction regressions (main session).

## Focused Gate results

- Pre-capability native recovery Gate: `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::codebuddy::recovery::tests -- --skip disabled_missing_cli_refresh_retains_recovery_authority --nocapture` — PASS, 15 tests, exit 0. This completed Job/startup implementation evidence before enabling capability.
- Enabled `canRecover = cfg!(windows)` only after that PASS; execute/continue/cancel/activity/tokenUsage remain false. Existing catalog fixture changes only `canRecover`; `availableForNewExecution` remains false.
- First entire CodeBuddy run: 64 passed / 1 fixture failed. Test held a cloned Registry Arc across existing exclusive startup ownership (`Arc::try_unwrap`), so the fixture now drops that read-only clone before startup; production TaskManager semantics unchanged.

## Final implementer focused result

`C:/Users/lifei/.cargo/bin/cargo.exe test --manifest-path src-tauri/Cargo.toml --lib agent::codebuddy -- --nocapture`

- PASS, exit 0: **65 passed, 0 failed, 0 ignored**, 1306 filtered; test execution 4.19s.
- Includes 16 new CodeBuddy recovery tests, real native Job zero/destroyed/live-policy/process-tree, disabled + unavailable discovery + refresh + actual Store reopen through TaskManager, and previous launcher/runtime/client/provider/discovery regressions.
- Existing `isolated_launcher_contract` without fixture env returns immediately, but the companion `launcher_contract_with_real_child_tree` compiles and invokes that isolated test process with env and executes the native assertions. New recovery native-tree test has no skip condition and executed successfully.
- Windows native rustc fixture used PATH (command environment prepended `C:\Users\lifei\.cargo\bin`), no user-specific compiler path is embedded in source.
- Focused failure history above is retained honestly; final verification used repaired fixtures and all cases passed.
- Source formatting: `rustfmt --edition 2024 --config skip_children=true` applied only to delivery-owned Rust paths, so nested frozen files were not recursively formatted.
- Source handed to main for remaining final verification and independent read-only full review. No commit/push.

## Implementation file inventory

New: `agent/codebuddy/recovery.rs`, `agent/codebuddy/recovery_windows.rs`, `agent/codebuddy/recovery/tests.rs`, `agent/store/codebuddy_runtime.rs`, `tests/fixtures/codebuddy_recovery_child.rs` (all under src-tauri).

Modified: `agent/codebuddy/mod.rs`, `agent/codebuddy/provider.rs`, `agent/codebuddy/provider/tests.rs`, `agent/codebuddy/client_tests.rs`; `agent/store.rs`, `agent/store/codebuddy.rs`, `agent/store/transactions.rs`, `agent/store/transactions/tests.rs`; `agent/task_manager.rs`, `agent/task_manager/recovery.rs`; `agent/codex/macos_recovery.rs` (only two scoped selectors); `agent/product/provider_catalog_tests.rs`, `agent/product/fixtures/provider_catalog_codex.json` (existing capability expectation).

No migration/schema, Codex Runtime/evidence/usage, UI/MCP field, result recovery or fresh execution implementation changed. Task-only implementer evidence: this file and `recovery-matrix.md`.

## Final Clippy follow-up

Main final regression results: fmt/check PASS; Codex runtime 17, TaskManager 29, Store transactions 50, catalog 4 PASS. Final Clippy found a new `unnecessary_literal_unwrap` in non-test Win32 observer because test injection's production `Option` was a literal None. Fixed only `recovery_windows.rs`: production obtains raw handle/error directly with Win32; test gets injected/raw result; both feed the same `open_error != ERROR_FILE_NOT_FOUND` classification. No lint suppression or recovery semantics change. Main reruns affected final checks before freezing.
