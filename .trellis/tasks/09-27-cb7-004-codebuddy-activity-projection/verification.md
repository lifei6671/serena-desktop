# Verification plan and environment

Status: implementation and final scoped verification complete; independent review pending freeze.

Baseline: feat/codebuddy, c890919ff8cfedab6f73682f257ff1cc889d9027, clean. Windows native commands run from E:/wx_lifeilin/github.com/lifei6671/serena-desktop/src-tauri with C:/Users/lifei/.cargo/bin/cargo.exe and that directory prepended to PATH for fixture rustc. CI .github/workflows/ci.yml prescribes fmt/check/clippy/test; use focused Rust targets appropriate to this card.

Required checks: focused CodeBuddy activity/prompt, full codebuddy (includes fresh and recovery), agent::telemetry_projector, relevant Store activity/telemetry, agent::task_manager, agent::codex, fmt --all -- --check, check --lib --tests, clippy --lib --tests -- -D warnings, git diff --check and scope/hash verification. Logs and commands/exits saved alongside this file. No real CodeBuddy/CB5 probe.

Linux: UNAVAILABLE. git ls-files '*docker*' '*Docker*' '*compose*' returned no project runner. No WSL invocation and no claim of Linux validation.

CodeGraph: registered workspace discovery returned 'MCP tool call requires approval, but approval policy is never'; local source used without repeating the blocked request.

## Baseline attribution

Previous committed CB7-003 verification and baseline-regressions.json record isolated baseline reproduction at 8499fef2378fdf18ec61ef6a472b2bc485491ed5:
- TaskManager authority_state_failure_uses_stable_internal_code: unwrap_err received Ok([]), recovery/tests.rs:207.
- Codex trait_startup_reconcile_projects_shared_recovery_errors_to_operation_failed: unwrap_err received empty ProviderReconcileSummary, adapter_tests.rs:1592.
- Clippy usage_tests.rs:987 await_holding_lock; awaits 1012:46/1016:55.

Current read-only git diff 8499fef2378fdf18ec61ef6a472b2bc485491ed5 c890919 -- src-tauri/src/agent/task_manager src-tauri/src/agent/codex src-tauri/src/agent/store/usage_tests.rs is empty. Baseline usage_tests hash is in baseline.json. Re-run current broad suites, preserve actual failures and verify unchanged relevant sources; do not suppress/repair these out-of-scope tests. Prior evidence is repository evidence, not a new baseline execution in this task.

## Final Windows results

Exact commands/cwd/exit codes: validation-results.jsonl. Cargo commands below use --locked and --lib (fmt uses --all). Counts overlap, do not sum.

| Check | Result | Evidence |
|---|---|---|
| test agent::codebuddy::activity | PASS exit0: 6 passed after Host correction | host-fix-focused.log; initial status matrix includes 16 cases |
| test agent::codebuddy::prompt::tests::activity_tests | PASS exit0: 7 passed | host-fix-prompt-activity.log |
| test codebuddy -- --nocapture --test-threads=1 | PASS exit0: 111 passed, 0 failed/ignored after Host correction | host-fix-codebuddy-serial.log; includes 13 new mapper/Prompt activity tests, CB7-003 Prompt, CB7-002 Fresh, CB6-005 recovery and Store/provider matrix |
| test agent::telemetry_projector | PASS exit0: 3 passed after Host correction | host-fix-telemetry.log |
| test activity, PATH includes Node | PASS exit0: 56 passed, 0 failed, 1 existing ignored after Host correction | host-fix-activity.log; includes Store, product, Codex and CodeBuddy activity |
| test agent::task_manager | FAIL exit101: 47 passed, 1 baseline failure, 1 existing ignored | task-manager.log; exact orphan-Claim failure above |
| test agent::codex | FAIL exit101: 149 passed, 1 baseline failure, 6 existing ignored | codex.log; exact orphan-Claim failure above |
| fmt --all -- --check | PASS exit0 after Host correction | host-fix-fmt.log |
| check --locked --lib --tests | PASS exit0 after Host correction | host-fix-check.log |
| clippy --locked --lib --tests -- -D warnings | FAIL exit101 after Host correction: only frozen baseline await_holding_lock | host-fix-clippy.log |
| git diff --check | PASS exit0 | scope check; no staged changes |

Clippy exact location src/agent/store/usage_tests.rs:987:9; awaits :1012:46, :1016:55. SHA256 C90595FCE0E1D57460648E3E28B2DC33A2C6E6AE7656C39C90AE96980958A778 remains identical to baseline. No allow/skip/fix added. Public telemetry/port, CodeBuddy provider and ExecutionTelemetryProjector hashes also unchanged (frozen-contract-baseline.json).

Activity first broad attempt: FAIL 41 passed/11 failed/1 ignored solely because node was absent from process PATH (program not found at product/tests.rs:2489). Existing C:/nvm4w/nodejs/node.exe and node_modules were verified; adding Node to command-local PATH produced 52/0/1 above, without install or source changes. Preserve activity-regressions.log as failed attempt evidence.

Implementation attempt: 93/94 CodeBuddy module tests passed; one new release_evidence assertion expected unknown instead of existing incomplete. Only test expectation repaired; dedicated rerun 1/1, pre-review full suite 107/107, and pre-Host post-review-repair suite 110/110 PASS. Logs implement-focused.log / implement-projector-rerun.log / codebuddy.log / repair1-codebuddy.log. Native linker stdout warning is library creation output, not a test failure.

## Requirement trace and scope

- activity/tests.rs: full kind/title matrix, initial Pending/InProgress versus Completed/Failed phases and retained category across all 16 status/kind cases, incremental category/status/streaming drop, bounded map, identity/parse failures, message privacy/forged fields, exact Host fixture preservation.
- prompt/activity_tests.rs: real projector exact activity columns/summary, full non-Activity authority snapshots, wrong-execution sink, injected projection failure, gated async Prompt projection, private requestId never taken from activity, four-field recording snapshots, wrong/early/late identity, slow pending sink cancellation with unchanged result/terminal.
- Existing prompt tests preserve ProviderRunResult snapshots, stop reasons, permission behavior, failure/caller-drop and stale revision behavior; unchanged provider tests freeze public execute/capabilities.
- Production changes only CodeBuddy private mapper, prompt telemetry and module registration; tests/fixtures confined to same feature. No Store/public contract/schema/capability/terminal transaction/Claim release/Usage changes.
- Best effort limit: FIFO <= queue_count plus one in-flight publication; map <= queue_count. Saturation or response drops pending events. Final drain skips publication if a prior publish was pending; otherwise polls sequentially until the first Pending and discards the rest. At most one irreversible Store submission remains in flight, preventing reordered final writes. Already submitted Store transactions retain existing sink semantics; no sink method starts or polls after final mapping boundary. No real probe and no Linux claim.
- Spec synchronization: no new generic contract or convention; existing §19 and public contracts remain authoritative. Card-specific frozen mapping and resource semantics documented in this task only.

## Independent review repair 1

review-round-1.md records one P2 final-drain ordering finding, accepted for repair. Only prompt.rs and prompt/activity_tests.rs changed: preserve pending-publication state across response boundary; skip final submissions if one was in flight; stop idle final publication after first Pending. Three new deterministic tests simulate submissions surviving future drop and reversed completion, prior in-flight submission, and immediately-ready wire order. Focused Prompt command `cargo test --manifest-path src-tauri/Cargo.toml agent::codebuddy::prompt --lib -- --nocapture` from repo root: PASS 15/15, exit0 (implement-repair1-prompt.log). Affected full suites and fmt/check rerun as above. TaskManager/Codex scopes unchanged by repair; their current-turn evidence remains applicable. No test weakening or public/Store contract repair.

## Host correction and verification follow-up

Same delivery unit resumed; pre-host-fix-* artifacts preserve superseded target B68A6632F495AFDD6DE6201F733579578DDBF1D04062C4CCA5439B8602F675D4 and its review. Only activity.rs/activity/tests.rs source hashes changed. Initial typed Completed/Failed now return Provider instead of Tool, while keeping structured category in bounded cache; Pending/InProgress and no-text inference remain unchanged. New current target requires fresh independent FULL_SCOPE with P0-P3=0 before completion.

First default-parallel full CodeBuddy attempt: FAIL exit101, 109 passed/2 failed. continuously_drains_bounded_collector failed with QueueBytes at prompt/tests.rs:293 (fixture limits 1024 bytes, 100ms TTL, message-only stream); managed_job_handshake_and_failure_cleanup failed EOF fixture write_all unwrap_err at runtime_tests.rs:115 (received Ok). Both paths bypass the changed initial ToolCall branch; their source and relevant Prompt/Runtime implementation are unchanged from the prior passing target. The same final source then ran the entire suite once with --test-threads=1: PASS 111/0/0 in 26.44s. No input/timeout/assertion/skip or product changes were made to obtain this pass. This supports a timing-sensitive fixture failure diagnosis, not proof of a root cause; parallel failure remains recorded in host-fix-codebuddy.log. Further broad repeats stopped after this successful full-suite verification.

All Host-correction commands had existing Node in process PATH from the outset. TaskManager/Codex historical baseline checks are not rerun for this two-file correction; the earlier results above remain labeled failures from the same delivery unit. Public contracts/provider/projector and usage_tests hashes were rechecked unchanged. No Linux or real CodeBuddy probe.
