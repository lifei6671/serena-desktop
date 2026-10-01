# Failures and Non-PASS Evidence

## 1. P1 startup recovery silently skips corrupt Claim authority — FIXED_IN_CB10_SAME_WORK

- Classification: `REGRESSION_OWNED_BY_PHASE_6`.
- Owner: CB6-005 — Runtime Termination Evidence / Startup Reconcile.
- Introducing commit: `ecd6c07 feat(agent): add CodeBuddy startup recovery`.
- Failing command: B1, exit 101, with 1432 passed / 2 failed / 27 ignored.
- Stable exact reruns: B1a and B1b both exit 101, each 0 passed / 1 failed / 0 ignored. This is not flaky.
- Failed tests:
  - `agent::codex::provider::adapter_tests::trait_startup_reconcile_projects_shared_recovery_errors_to_operation_failed`
  - `agent::task_manager::recovery::tests::authority_state_failure_uses_stable_internal_code`
- Minimal runtime evidence: both expected an error for a persisted `workspace_claims` row whose Execution is missing; current code returned an empty successful reconcile result (`ProviderReconcileSummary { items: [] }` / `[]`).
- Attribution evidence:
  - Both tests predate the CodeBuddy branch change (`git blame` points to `d8d18336`, 2026-09-16).
  - Before `ecd6c07`, `recover_claims()` selected every persisted Claim and the existing `load()` path returned `EXECUTION_NOT_FOUND` for the dangling row.
  - `ecd6c07` added `recover_provider_claims()` with `WHERE EXISTS (SELECT 1 FROM executions ... provider=?1)`, so the missing-Execution Claim is filtered out before authority classification.
  - CB6-005's recorded focused matrix ran `agent::task_manager::tests`, not `agent::task_manager::recovery::tests`, and did not run the Codex provider adapter group; the regression therefore escaped its targeted Gate.
- Impact: provider startup recovery can publish success while an authoritative persisted Claim is structurally inconsistent and unreconciled. The Claim is not released, but the required stable state failure is hidden. This is fail-open authority handling and blocks CB10-001.
- Repair: `FIXED_IN_CB10_SAME_WORK` after Host approval.
  - `src-tauri/src/agent/store/transactions.rs`: restore Claim-only ordered scan; execute existing `load(tx, id)?` before a requested-provider mismatch can read-only `continue`.
  - `src-tauri/src/agent/store/transactions/tests.rs`: add bidirectional legal-provider skip with unchanged Execution/Claim/Runtime/private state, own-provider classification, and generic/Codex/CodeBuddy dangling-Claim error equivalence.
- Closure evidence:
  - Both original failures exact single-thread: 1/1 PASS each.
  - New provider-scoped matrix: 3/3 PASS.
  - Store transactions: 57/57 PASS.
  - task-manager recovery: 18 PASS / 1 explicit real-binary ignored.
  - Codex adapter: 28/28 PASS; CodeBuddy recovery: 21/21 PASS.
  - Full serial Rust: 1437 PASS / 0 FAIL / 27 ignored.

## 2. Strict all-target Clippy baseline

- Classification: `PRE_EXISTING_BASELINE_FAILURE`.
- Command: A5, exit 101.
- Only diagnostic: `src-tauri/src/agent/store/usage_tests.rs:987` `clippy::await_holding_lock`, across awaits at lines 1012 and 1016.
- The file was not changed by CB10-001. Post-repair `cargo clippy --lib -- -D warnings` passed; post-repair all-target rerun again produced only this diagnostic.
- Repair: NOT ATTEMPTED; explicitly out of scope.

## Environment limitations

- `ENVIRONMENT_MISCONFIGURED`: the first R3 Codex exact-filter launch failed before process creation with `CreateProcessWithLogonW failed: 267`; exit code was unavailable and zero tests started. One retry of the identical command was allowed because this was a transient launcher failure; R3 then passed 1/1. This attempt is not test evidence and is retained in `evidence/raw/R3-pre-codex-launcher-failure.log`.
- `ENVIRONMENT_MISCONFIGURED`: R13 invoked Cargo by absolute path but did not prepend the approved `C:\nvm4w\nodejs` directory to the child `PATH`. Node/npm-dependent Product and MCP tests therefore returned `program not found`, producing 1358 pass / 79 fail / 27 ignored. With the command environment corrected, exact R14 passed 1/1 and authoritative full R15 passed 1437/0/27. R13 remains a recorded failed command, not regression evidence.
- `ENVIRONMENT_UNAVAILABLE`: repository-provided Docker/Linux runner and Docker CLI were not found. WSL was not used and cannot substitute for Linux evidence.
- Native macOS validation is unavailable on this Windows host.
- Trellis Python runtime is unavailable, so task creation/start scripts could not run; artifacts were created manually in the established repository format.
- Node/npm and Cargo were available by the approved absolute paths; no dependency install or upgrade was performed.
