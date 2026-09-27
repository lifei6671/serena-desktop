# CB7-002 verification

Baseline/current HEAD: `5e2ff1e21999a8888b29af02e4dca2773d27efe5`, branch `feat/codebuddy`; initial clean. Native Windows x86_64-pc-windows-msvc. Cargo executable `C:/Users/lifei/.cargo/bin/cargo.exe`; Cargo cwd `E:/wx_lifeilin/github.com/lifei6671/serena-desktop/src-tauri`. Command process PATH prepends `C:\Users\lifei\.cargo\bin` so existing native fake tests can invoke rustc; product child environment remains the existing resolver projection.

| Exact Cargo arguments | Result | Evidence |
| --- | --- | --- |
| `test --lib codebuddy -- --nocapture` | PASS exit 0: 89 passed, 0 failed/ignored | codebuddy-release.log |
| `test --lib agent::task_manager -- --nocapture` | FAIL exit 101: 47 passed, 1 failed, 1 ignored | task-manager-release.log |
| `test --lib agent::codex -- --nocapture` | FAIL exit 101: 149 passed, 1 failed, 6 ignored | codex-release.log |
| `fmt --all -- --check` | PASS exit 0 | fmt-release.log |
| `check --lib --tests` | PASS exit 0 | check-release.log |
| `clippy --lib --tests -- -D warnings` | FAIL exit 101: only frozen baseline diagnostic | clippy-release.log |

`release-results.jsonl` records exact commands/cwd/exits. Counts are per suite, not unique totals; CodeBuddy-filtered tests include Store, CB6-003 transport, CB6-005 recovery, CB7-001 catalog and focused TaskManager coverage. No skipped test is counted as passed. Native fake tests use isolated temporary executables and real Job/SQLite, not real CodeBuddy. The native ORDER trace ends `session durable -> route register -> early replay -> config ACK -> acceptance_ready -> accepted -> STOP`; prompt count is zero.

## Independently reproduced baseline failures

Created an isolated source copy with `git archive --format=zip --output=<Temp>/serena-cb7-002-baseline-5e2ff1e.zip 5e2ff1e21999a8888b29af02e4dca2773d27efe5`, then `Expand-Archive`. No working-tree source was rolled back. Baseline cwd: `C:/Users/lifei/AppData/Local/Temp/serena-cb7-002-baseline-5e2ff1e/src-tauri`. Each exact command used the same Cargo executable and `test --lib --target-dir E:/wx_lifeilin/github.com/lifei6671/serena-desktop/src-tauri/target <filter> -- --nocapture`.

1. `authority_state_failure_uses_stable_internal_code`: baseline FAIL exit 101, 0 passed/1 failed; same `recovery/tests.rs:207` unwrap_err on `Ok([])` as current. Log `baseline-authority_state_failure_uses_stable_internal_code.log`.
2. `trait_startup_reconcile_projects_shared_recovery_errors_to_operation_failed`: baseline FAIL exit 101, 0 passed/1 failed; same `adapter_tests.rs:1592` unwrap_err on empty `ProviderReconcileSummary` as current. Log `baseline-trait_startup_reconcile_projects_shared_recovery_errors_to_operation_failed.log`.

Both fixtures insert a Claim whose Execution is missing. Frozen provider-scoped recovery filters by an existing Execution with the requested provider, so it selects neither orphan Claim. Related production code and both tests are unchanged. They are reported FAIL, not waived/disabled/repaired by this task.

Clippy first/only final diagnostic: `src/agent/store/usage_tests.rs:987:9`, `clippy::await_holding_lock`; awaits at 1012:46 and 1016:55. File SHA256 remains `C90595FCE0E1D57460648E3E28B2DC33A2C6E6AE7656C39C90AE96980958A778`, matching baseline.json. No suppressions or unrelated fixes.

## Scope and limits

Repo-root `git diff --check`: PASS exit 0, diff-check-release.log. scope-check.json checks all 840 frozen files: exactly 10 tracked implementation/test files changed, 4 Rust files added, remaining frozen files unchanged. Cargo/schema/migrations/CB5 evidence/design authority/Codex production source unchanged. No staged changes, commits, pushes, real CodeBuddy probe, or CB7-003 prompt/result implementation. Broad regression/Clippy gates remain partial due the proven baseline failures; current CodeBuddy acceptance tests pass.

Linux: ENVIRONMENT_UNAVAILABLE, no project-provided Docker Desktop runner found. No WSL validation or host-result substitution. macOS: NOT_RUN. Trellis CLI could not run because python was absent and py reported no installed Python; task artifacts were created manually. Spec review found no backend-specific spec directory to update; the scoped Host decision is captured in this task's design/acceptance records.

Temporary baseline cleanup command was rejected by automatic approval policy (`blocked by policy`). The named baseline directory and zip remain in system Temp; they are outside the source worktree. No permission bypass was attempted.

## Historical diagnostics retained

Original MCD diagnostic PASS established SDK loss of models; Host resolved it and current positive capture test replaces it. Early logs also retain corrected fixture failures: generic `default.exe` exited before main with `0xffffffff`; prefixed `cb7-fresh-*` fixtures passed, underlying environment cause not claimed. Workspace fault construction initially hit FK constraints, then verbatim PathBuf::join normalized away the intended mismatch; current fixture deliberately constructs the raw invalid path. Two hung diagnostic runs were stopped, not counted as PASS. Initial formatting/Clippy findings were repaired; only final release logs represent final verification.

## Independent review repair round 1

Initial FULL_SCOPE reviewer verified target `481FDB4A3FC941BDCD6E72E98C89D2B3F55CF32A615D1ADC564DFC6E4D2E151B`, all 14 sources and 39 context hashes, and found no P0/P1. One P2 was repaired: a typed config-option ACK changing category=mode now reconciles legacy modes.currentModeId, without requiring an additional notification. A native fake regression verifies exact config id/value, successful real acceptance, and zero prompt. Existing contradictory catalog rejection remains.

Final repair command `cargo test --lib codebuddy -- --nocapture`: PASS exit 0, 90 passed, 0 failed/ignored (review-repair-tests.log); same Cargo cwd and PATH as above. Original 89-test run remains historical evidence. Broad TaskManager/Codex baseline failures are unchanged; this repair changes only fresh catalog ACK reconciliation and its regression.
`cargo fmt --all -- --check`: PASS 0; `cargo check --lib --tests`: PASS 0; `cargo clippy --lib --tests -- -D warnings`: FAIL 101, only unchanged usage_tests.rs:987 await_holding_lock. Evidence: review-repair-fmt.log, review-repair-check.log, review-repair-clippy.log. git diff --check: PASS 0.
