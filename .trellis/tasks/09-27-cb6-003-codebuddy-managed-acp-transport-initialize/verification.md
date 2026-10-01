# CB6-003 verification history and final evidence

Historical pre-implementation checkpoint: no Rust or dependency file had been changed and no focused test/build had been executed. The initial blocker is RESOLVED_BY_HOST. This first matrix is retained as history; resumed and final results follow below.

| Contract | Current result |
|---|---|
| Initialize success; exact raw/typed protocol v1 | NOT_RUN |
| Mismatch -> CODEBUDDY_ACP_INCOMPATIBLE; optional global Unavailable | NOT_RUN |
| Missing capability -> health unchanged; all six public capabilities false | NOT_RUN |
| EOF before/during initialize; timeout; invalid NDJSON/malformed JSON-RPC | NOT_RUN |
| Multiple pending/out-of-order; unknown/duplicate response ids | NOT_RUN |
| Notification before response | NOT_RUN |
| Early session/config_option_update before synthetic session/new; ordered replay | NOT_RUN |
| Wrong-session isolation and close queue cleanup | NOT_RUN |
| Queue count/bytes/frame size/TTL; pending count/request timeout bounds | NOT_RUN |
| Bounded private stderr tail and public error isolation | NOT_RUN |
| Permission fail closed; unsupported server id request -32601 | NOT_RUN |
| Notification without id produces no response | NOT_RUN |
| Shutdown fails every pending waiter | NOT_RUN |
| Initialize failure/post-create failure converges owned Job | NOT_RUN |
| CB6-002 Windows launcher ownership regression | NOT_RUN |
| Codex regression if shared dependency impact requires | NOT_RUN; dependency change absent |

Planned commands, cwd `src-tauri`, executable `C:\Users\lifei\.cargo\bin\cargo.exe`:

- `cargo test --lib agent::codebuddy:: -- --nocapture`: NOT_RUN, implementation/dependency blocked.
- `cargo fmt --all -- --check`: NOT_RUN, no Rust changes.
- `cargo check --lib`: NOT_RUN, implementation/dependency blocked.
- `cargo clippy --lib --tests -- -D warnings`: NOT_RUN. CB6-002 verification records historical `src/agent/store/usage_tests.rs:987` await_holding_lock; not reverified and not presented as this turn's Clippy result.

Windows-only work; Linux/Docker/WSL validation NOT_RUN. No WSL invocation. No real CodeBuddy/CB5 probe. No health updates.

Environment command evidence: `research/dependency-environment.md`.
Delivery verdict: UNABLE_TO_VERIFY; ImplementationComplete=false. Production coverage is absent, regardless of any handoff-document audit result.

Scope check: PASS — git diff --name-only and --cached empty; git status --short --untracked-files=all contains only this task directory. git diff --check: PASS (exit 0; tracked diff empty, not proof of executable tests).

Resume: prior dependency blocker RESOLVED_BY_HOST. Above NOT_RUN matrix is historical pre-implementation state; subsequent results will be appended. Prior document-only review is historical and cannot approve the upcoming implementation.

## Resumed verification

- Trellis task.py validate using explicit Python312: PASS, exit 0. It warns the full task-breakdown source exceeds injection size; task prd.md preserves the complete CB6-003 acceptance locally and reviewer receives full user scope.
- Native Windows cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check: PASS, exit 0 after formatting only delivery-owned Rust files.
- Product cargo check --lib --tests via existing bootstrap: RUNNING, dependency index resolved and product crate downloads ongoing. Exact log check-first.log. No compile/test result yet.
- Independent source checkpoint requested for checkpoint-target.json; final gate awaits tests and any corrections.

First product check attempt: UNAVAILABLE before compilation, bootstrap 600-second watchdog expired during successful product crate downloads. check-first.log preserves TimeoutExpired and taskkill exit 1 / descendantsReaped=unverified. The enclosing command exited 1. Follow-up Get-Process cargo returned no running Cargo process; no compiler had started according to the log. CIM process details are unavailable (access denied). Retain failed cleanup report; no claim that taskkill succeeded. A second --locked attempt resumes the now-populated index/download cache after observing no live Cargo process; no source/cache was manually modified.

Second bootstrap product check: FAILED, exit 101 before product Rust compilation. aws-lc-sys MSVC C1083 at a 278-character existing source path. Source existence was verified; no dependency API difference. A temporary junction alias was ineffective because source paths resolved to the original long path (check-short-path.log, exit 101). No source workaround/patch applied.

A separate short temporary Cargo home was populated by read-only copies of original registry archive/index files; Cargo expands these itself offline. Original task-local cache is not manually edited. check-offline.log uses --offline --locked and the same source replacement identity. Exact cache path/provenance in temporary-build-cache.json. This is native Windows validation, no Linux/WSL claim.

Cleanup limitation: automatic approval review rejected both the combined cleanup/preparation command and a narrowed, exact-literal, verified-junction-only Remove-Item command, returning only blocked by policy. No alternate deletion mechanism attempted. The unused junction recorded in temporary-alias.json remains; it points to the existing CB5-002 cache and contains no independent copy. The independent short build cache is currently needed for remaining checks.

## EOF narrow repair continuation

- Previous Host-cancelled compile `tests-runtime-eof-diagnostic.log` has no terminal result; NOT_COMPLETED, not a pass.
- `tests-runtime-pipe-kind.log`: native Windows single Runtime test FAILED (exit 101). Strict EOF assertion reports actual `Some(Timeout)`, not `Io`. Test-only writer diagnostics emitted no ErrorKind. Therefore broken-pipe-first remains a hypothesis until observed.
- Narrow fixture correction under validation: remove inherit flag from the fake parent's ACP std handles before spawning its still-owned descendant, and explicitly wait for parent exit for EOF-before. Parent/descendant first-operation Job membership and final exact-zero Job accounting remain mandatory. Production launcher/Job ownership unchanged.
- `tests-runtime-pipe-kind-isolated.log`: PASS 1/1 (exit 0); all seven Runtime modes passed after fixture handle isolation. This establishes that the earlier Timeout did not prove a production write-side Io race.
- `tests-runtime-native-pipe-kind.log`: PASS 1/1 (exit 0), with direct write to the exact owned stdin after peer process exit reporting `BrokenPipe`, Win32 code 232. No provider content recorded.
- Narrow production mapping now maps only `ErrorKind::BrokenPipe` to `Failure::Eof` at read/write/flush boundaries. Other I/O kinds stay `Failure::Io`; Shared first-failure is unchanged. Temporary test-only prints removed. Native EOF fixture keeps strict `Err(Eof)` and direct ErrorKind assertion. Added write/flush injection coverage for BrokenPipe/Other/TimedOut and first-failure/health invariance.
## Final verification results (native Windows, delivery target pending review)

Cwd for commands: repository root. PATH explicitly prepends C:\Users\lifei\.cargo\bin. CARGO_HOME is the short independent cache recorded in temporary-build-cache.json. Cargo test/check use `--config 'source.crates-io.replace-with="task-official"' --config 'source.task-official.registry="sparse+http://127.0.0.1:18742/"'` plus `--offline --locked --manifest-path src-tauri/Cargo.toml`; Clippy requires these options AFTER `clippy`. No live proxy/network or real provider is used by offline validation.

| Command (remaining arguments) | Result | Evidence |
|---|---|---|
| test --lib agent::codebuddy::runtime::tests:: -- --nocapture | PASS, exit 0, 1/1; all 7 modes | tests-runtime-repaired.log |
| test --lib agent::codebuddy:: -- --nocapture | PASS, exit 0, 49/49, 0 ignored | tests-codebuddy-repaired.log |
| cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check | PASS, exit 0 | fmt-final.log |
| check --lib --tests | PASS, exit 0 | check-final.log |

Earlier 47/48 runs and all fixture diagnostic failures remain in their original logs; they are superseded, not deleted. `clippy-strict.log` failed offline dependency resolution (configuration placed before external subcommand); this is not a lint result. Corrected invocation is recorded separately in clippy-strict-configured.log.

Contract matrix, all PASS in 49/49 run:
- initialize sanitized success and protocolVersion=1 exact raw/typed; deterministic mismatch only health_change=Unavailable; absent capabilities keep health unchanged and all six caps false.
- EOF before/during initialize; closed native Windows peer stdin ErrorKind=BrokenPipe; write and flush BrokenPipe/Other/TimedOut injection; first-failure preserved and local health classification.
- initialize/request timeout, invalid NDJSON, malformed JSON-RPC (including SDK i32 error code domain), temporary I/O, abandoned request, shutdown all pending.
- multiple pending out-of-order exact ids; unknown/duplicate ids diagnosed; notification before response; SDK pre-handler control notifications ignored without blocking.
- synthetic session/new response preceded by early config_option_update, exact route replay order, wrong-session isolation, close cleanup.
- single frame/outgoing frame limits, pending count, cumulative queue count/bytes with reclaim, TTL, route count and backpressure.
- fail-closed request_permission, unsupported id request -32601, no-id notification no response, bounded private stderr tail.
- real Win32 pipes with fake executable: success/mismatch/timeout/eof/drop/post-create/transport failure; first-operation membership and exact Job ActiveProcesses==0 cleanup. Existing CB6-002 launcher tests are included in the 49/49 run.

Evidence boundary: only fake processes and sanitized/synthetic fixtures executed. CB5-002 initialize wire is pre-existing evidence; early ordering fixture uses Host observation with synthetic sessionId/content (research/sdk-and-fixtures.md). No real session/new or prompt, no new Host real-provider claim. Linux/Docker not run; no WSL.

Final checks after EOF repair:
| Command / check | Result | Evidence |
|---|---|---|
| clippy --lib --tests -- -D warnings (configured offline) | FAIL exit 101, exactly one pre-existing lint: src/agent/store/usage_tests.rs:987 await_holding_lock (awaits 1012/1016); file unchanged from baseline | clippy-strict-configured.log |
| clippy --lib -- -D warnings | PASS exit 0 | clippy-lib.log |
| clippy --lib --tests -- -D warnings -A clippy::await_holding_lock | PASS exit 0; baseline-lint exclusion is NOT a strict full-suite pass | clippy-baseline-excluded.log |
| test --lib agent::codebuddy::windows_launcher:: -- --nocapture | PASS exit 0, 8/8 | tests-launcher-final.log |
| test --lib agent::codex::windows_launcher:: -- --nocapture | PASS exit 0, 3/3 | tests-codex-launcher.log |
| test --lib agent::codex::runtime:: -- --nocapture | PASS exit 0, 17/17 | tests-codex-runtime.log |
| git diff --check / narrow scope / staged empty / HEAD unchanged | PASS | diff-check.log, scope-status.txt |

The strict Clippy baseline exception is permitted by the user's task (record existing blocker, do not falsely claim pass). No other lint occurred; no baseline source was changed or suppressed in code. Native linker emits a library/export informational warning during test linking; all test commands still exit 0. Shared dependency regression is limited to the two existing Codex process modules above, whose source is unchanged. No broader tests are required without a new defect or edit.

Implementation and required verification are complete. Delivery remains in_progress until independent final full review and matching final freshness/coverage. No Material Contract Difference identified.
