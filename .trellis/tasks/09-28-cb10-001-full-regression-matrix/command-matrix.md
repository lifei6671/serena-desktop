# Command Matrix

Statuses: `PASS`, `FAIL`, `NOT_RUN`, `ENVIRONMENT_UNAVAILABLE`, `ENVIRONMENT_MISCONFIGURED`.

| ID | Command | Exit | Actual tests (pass/fail/ignored/0-test) | Status | Classification | Owner | Raw evidence |
|---|---|---:|---|---|---|---|---|
| A1 | `git diff --check` | 0 | n/a | PASS | — | CB10-001 | `evidence/raw/A1-git-diff-check.log` |
| A2 | `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 0 | n/a | PASS | — | CB10-001 | `evidence/raw/A2-cargo-fmt.log` |
| A3 | `cargo check --manifest-path src-tauri/Cargo.toml` | 0 | n/a | PASS | — | CB10-001 | `evidence/raw/A3-cargo-check.log` |
| A4 | `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings` | 0 | n/a | PASS | — | CB10-001 | `evidence/raw/A4-cargo-clippy-lib.log` |
| A5 | `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | 101 | n/a | FAIL | `PRE_EXISTING_BASELINE_FAILURE` | historical Usage test | `evidence/raw/A5-cargo-clippy-all-targets.log` |
| B1 | `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` | 101 | 1432 pass / 2 fail / 27 ignored / nonzero | FAIL | `REGRESSION_OWNED_BY_PHASE_6` | CB6-005 | `evidence/raw/B1-cargo-test-lib-serial.log` |
| B1a | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::codex::provider::adapter_tests::trait_startup_reconcile_projects_shared_recovery_errors_to_operation_failed -- --exact --test-threads=1 --nocapture` | 101 | 0 pass / 1 fail / 0 ignored | FAIL | `REGRESSION_OWNED_BY_PHASE_6` | CB6-005 | `evidence/raw/B1a-codex-reconcile-exact-rerun.log` |
| B1b | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::task_manager::recovery::tests::authority_state_failure_uses_stable_internal_code -- --exact --test-threads=1 --nocapture` | 101 | 0 pass / 1 fail / 0 ignored | FAIL | `REGRESSION_OWNED_BY_PHASE_6` | CB6-005 | `evidence/raw/B1b-task-manager-recovery-exact-rerun.log` |
| R1 | `cargo test --manifest-path src-tauri/Cargo.toml --lib provider_scoped_recovery_ -- --test-threads=1` | 0 | 3 pass / 0 fail / 0 ignored | PASS | `FIXED_IN_CB10_SAME_WORK`; final review-strengthened fixture | CB6-005 | `evidence/raw/R1-final-provider-scoped-matrix.log` |
| R2 | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::task_manager::recovery::tests::authority_state_failure_uses_stable_internal_code -- --exact --test-threads=1 --nocapture` | 0 | 1 pass / 0 fail / 0 ignored | PASS | `FIXED_IN_CB10_SAME_WORK` | CB6-005 | `evidence/raw/R2-task-manager-existing-exact.log` |
| R3-pre | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::codex::provider::adapter_tests::trait_startup_reconcile_projects_shared_recovery_errors_to_operation_failed -- --exact --test-threads=1 --nocapture` | n/a; process not started | 0 tests started | `ENVIRONMENT_MISCONFIGURED` | Windows launcher reported `CreateProcessWithLogonW failed: 267`; retried once | Host command launcher | `evidence/raw/R3-pre-codex-launcher-failure.log` |
| R3 | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::codex::provider::adapter_tests::trait_startup_reconcile_projects_shared_recovery_errors_to_operation_failed -- --exact --test-threads=1 --nocapture` | 0 | 1 pass / 0 fail / 0 ignored | PASS | `FIXED_IN_CB10_SAME_WORK` | CB6-005 | `evidence/raw/R3-codex-existing-exact.log` |
| R4 | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::store::transactions::tests -- --test-threads=1` | 0 | 57 pass / 0 fail / 0 ignored | PASS | final review-strengthened fixture | Store | `evidence/raw/R4-final-store-transactions-group.log` |
| R5 | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::task_manager::recovery::tests -- --test-threads=1` | 0 | 18 pass / 0 fail / 1 ignored | PASS | — | Runtime/Recovery | `evidence/raw/R5-task-manager-recovery-group.log` |
| R6 | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::codex::provider::adapter_tests -- --test-threads=1` | 0 | 28 pass / 0 fail / 0 ignored | PASS | — | Codex | `evidence/raw/R6-codex-adapter-group.log` |
| R7 | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::codebuddy::recovery::tests -- --test-threads=1` | 0 | 21 pass / 0 fail / 0 ignored | PASS | — | CodeBuddy | `evidence/raw/R7-codebuddy-recovery-group.log` |
| R8 | `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 0 | n/a | PASS | — | CB10-001 | `evidence/raw/R8-cargo-fmt-check.log` |
| R9 | `cargo check --manifest-path src-tauri/Cargo.toml` | 0 | n/a | PASS | — | CB10-001 | `evidence/raw/R9-cargo-check.log` |
| R10 | `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings` | 0 | n/a | PASS | — | CB10-001 | `evidence/raw/R10-cargo-clippy-lib.log` |
| R11 | `git diff --check` | 0 | n/a | PASS | — | CB10-001 | `evidence/raw/R11-git-diff-check.log` |
| R12 | `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` | 0 | 1437 pass / 0 fail / 27 ignored / nonzero | PASS | — | CB10-001 | `evidence/raw/R12-cargo-test-lib-serial-after-repair.log` |
| R13 | `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` with Node directory absent from child `PATH` | 101 | 1358 pass / 79 fail / 27 ignored / nonzero | `ENVIRONMENT_MISCONFIGURED` | Node/npm-dependent Product and MCP tests reported `program not found` | Host command environment | `evidence/raw/R13-full-rust-node-path-misconfigured.log` |
| R14 | `$env:PATH='C:\nvm4w\nodejs;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --lib mcp::orchestration_tests::start_routing_tests::continuation_routing_tests::continue_inherits_frozen_identity_while_query_reports_current_route -- --exact --test-threads=1 --nocapture` | 0 | 1 pass / 0 fail / 0 ignored | PASS | environment correction confirmed | Host command environment | `evidence/raw/R14-node-path-exact-rerun.log` |
| R15 | `$env:PATH='C:\nvm4w\nodejs;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` | 0 | 1437 pass / 0 fail / 27 ignored / nonzero | PASS | authoritative final full Rust result | CB10-001 | `evidence/raw/R15-cargo-test-lib-serial-final.log` |
| R16a | `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 0 | n/a | PASS | — | CB10-001 | `evidence/raw/R16a-cargo-fmt-check.log` |
| R16b | `cargo check --manifest-path src-tauri/Cargo.toml` | 0 | n/a | PASS | — | CB10-001 | `evidence/raw/R16b-cargo-check.log` |
| R16c | `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings` | 0 | n/a | PASS | — | CB10-001 | `evidence/raw/R16c-cargo-clippy-lib.log` |
| R16d | `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | 101 | n/a | FAIL | `PRE_EXISTING_BASELINE_FAILURE` | historical Usage test | `evidence/raw/R16d-cargo-clippy-all-targets.log` |
| R16e | `git diff --check` | 0 | n/a | PASS | — | CB10-001 | `evidence/raw/R16e-git-diff-check.log` |
| C1 | v9→v10→v11→v12→v13 migration/current-open/reopen tests inside B1 | inherited B1 | Store category 151 pass / 0 fail / 0 ignored | PASS | — | Phase 1 / Store | `evidence/raw/B1-cargo-test-lib-serial.log` |
| D1 | Runtime/Recovery/Claim suites inside B1 | inherited B1 | CodeBuddy 141/0/0; Codex 149/1/6; TaskManager 48/1/1 | FAIL | `REGRESSION_OWNED_BY_PHASE_6` | CB6-005 | `evidence/raw/B1-cargo-test-lib-serial.log` |
| E1 | Provider/routing/Product suites inside B1 | inherited B1 | Product 136/0/5; Provider 29/0/0; routing tests passed | PASS | — | Phases 1–4, 7–9 | `evidence/raw/B1-cargo-test-lib-serial.log` |
| F1 | `npm test` | 0 | 165 pass / 0 fail / 0 skipped | PASS | — | Frontend | `evidence/raw/F1-npm-test.log` |
| F1a | `C:\nvm4w\nodejs\node.exe --test src/AgentPanel.test.mjs` | 0 | 86 pass / 0 fail / 0 skipped | PASS | — | AgentPanel | `evidence/raw/F1a-agent-panel-full.log` |
| F2 | `npm run lint` | 0 | n/a; 0 errors / 5 pre-existing cache warnings | PASS | — | Frontend | `evidence/raw/F2-npm-lint.log` |
| F3 | `npm run build` (`tsc && vite build`) | 0 | n/a | PASS | — | Frontend | `evidence/raw/F3-npm-build.log` |
| F4 | `C:\nvm4w\nodejs\node.exe --test scripts/apply-release-version.test.mjs scripts/check-version.test.mjs scripts/ci-workflow.test.mjs scripts/release-workflow.test.mjs scripts/macos-bundle-config.test.mjs scripts/verify-installer.test.mjs scripts/verify-macos-release.test.mjs scripts/verify-uninstall-policy.test.mjs` | 0 | 35 pass / 0 fail / 0 skipped | PASS | — | Release scripts | `evidence/raw/F4-ci-release-installer-tests.log` |
| G1 | `cargo test --manifest-path src-tauri/Cargo.toml --lib mcp:: -- --test-threads=1` | 0 | 310 pass / 0 fail / 4 ignored; fixed remote surface=21 tools | PASS | — | MCP | `evidence/raw/G1-mcp-contract-group.log` |
| A5R | `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | 101 | n/a | FAIL | `PRE_EXISTING_BASELINE_FAILURE` | historical Usage test | `evidence/raw/A5-after-repair-clippy-all-targets.log` |
| H1 | PowerShell: `git branch --show-current; git rev-parse HEAD; git status --short; git diff --check; Get-FileHash` plus task-local large/binary/temp/secret/trailing-whitespace scans | 0 | n/a | PASS | only 2 executable files plus CB10 task-local artifacts; authority hashes unchanged | CB10-001 | `evidence/raw/H1-final-hygiene.log` |
| H2 | Independent `CHILD_AGENT / FULL_SCOPE` review of frozen Tier-3 delivery | n/a | review coverage complete | PASS | P0/P1/P2/P3 = 0; target fresh | code-delivery-review | `review-final.md` |

## Ignored/smoke accounting

The 27 ignored tests are not counted as PASS. They are explicit real-binary, native WebView2/autostart, live network/tunnel, official Serena installation, CodeGraph installed-index, or isolated destructive Host smokes. Their exact names and reasons are retained in `R12-cargo-test-lib-serial-after-repair.log`. No ignored test was force-run because this card forbids real destructive provider calls.

## Count notes

- B1 preserves the original failing evidence: `1432 passed; 2 failed; 27 ignored`.
- R12 was the initial post-repair full result: `1437 passed; 0 failed; 27 ignored; 0 measured; 0 filtered out`.
- After the review-driven test strengthening, R15 supersedes R12 as the authoritative final full result with the same `1437/0/27` count. R13 is retained as environment-failure evidence and is not treated as a test pass; R14 proves the corrected Node PATH on an exact former failure.
- R3-pre never created a process and therefore supplies no test result; the one permitted retry is separately recorded as R3 and passed 1/1.
- CodeBuddy Usage `SKIPPED_UNSUPPORTED` / `tokenUsage=false` remains expected and did not fail any executed test.
