# CB5-003 independent final review — runner repair round 1

## Identity / gate

- Reviewer: independent CHILD_AGENT `/root/review_probe`; Strict, Tier 3, read-only implementation review. Only this review record was written.
- Target: `602497b57db98907620c76721663e2fe9404f542d61d6706dab9b46265525abe`.
- Product HEAD: `5179248cfa30096df912c6567aee2e80e3bf4352`.
- Coverage: COMPLETE. Freshness: FRESH. All 85 frozen file SHA256 values independently match, zero mismatches.
- Harness implementation/evidence-integrity review gate: **PASSED**. Both prior P1 findings are RESOLVED. No remaining findings.
- Actual contract: **BLOCKED for this execution**. Formal contract delivery: **UNABLE_TO_VERIFY**. Harness review does not grant canExecute or Host Gate acceptance.

## Findings (fixed)

Repairs were made by the implementer and independently adjudicated here; the reviewer did not alter target code.

1. **RESOLVED — stale output reused as a fresh retry.** `run_probe.py` now gives each attempt a new TemporaryDirectory and result.json path. Missing current output saves scenario/attempt/exit diagnostic CURRENT_ATTEMPT_RESULT_MISSING and raises; no old wire/PID/cwd is copied. `test_missing_second_result_cannot_reuse_first` reproduces the original trigger, verifies unique paths, verifies no fabricated attempt-2 record, and checks the separate failure diagnostic.
2. **RESOLVED — earlier cleanup failure masked by a later result.** `attempt_clean()` requires exit 0, cleanup success, direct-child reaping, no cleanup errors, and workspace deletion. The execution path applies this before any retry/next scenario. Final aggregation applies it to every attempt, including summarize mode. Tests verify stop-before-retry for exit/cleanup/reaping/deletion failures, rejection when an earlier cleanup failure precedes successful final results, successful clean retry, and exactly one retry per scenario.

## Findings (not fixed)

None. No P0/P1/P2 remains in the current reviewed target.

## Coverage / integration

The preceding full affected review is `review-round-uuidv7-1.md`, target `17aae46ce9433f045ff0b808b9833d5d0ddd519f6ec0a59b789c00d7c97ad11e`. This pass reviewed the complete updated Python runner and five-test file, verification changes, repair provenance and target/history integration. Unchanged Rust source/package coverage is retained after matching all five hashes against the preceding target. Earlier role context, task contracts, protocol/process review and archive coverage remain applicable.

UUID generation remains `Uuid::now_v7().simple().to_string()` with v7 feature: exact 32 lowercase hexadecimal characters, no hyphens, version 7, exact `_meta["codebuddy.ai/conversationRequestId"]` serialization and A absence. Public correlation sanitization and response/result metadata versus notification params/update metadata are separately handled. Real echo/acceptance remains NOT_PROVEN.

Harness still owns canonical Node/CodeBuddy Child and pipes; official SDK 2.2.0 receives external streams. Protocol gate remains initialize success plus number 1. Temp workspace manifests, raw/typed separation, observed NDJSON order, empty identity fail-closed, bounded cleanup and fail-closed permission handling retain the preceding reviewed behavior. No product ActivityEvent mapping or product capability change was introduced.

## Real evidence provenance

Independently compared 10 real evidence files against the pre-repair frozen target: all byte-identical, including four attempt records, two final scenario records, 16-frame wire, process/activity summaries and correlation decision. The four real attempts occurred before the Python runner repair; no real executions occurred afterward. `history/pre-runner-repair/` preserves the producer runner and provenance. Current verification explicitly states this distinction rather than claiming repaired code produced earlier observations.

The previous independent pass verified four distinct PIDs/cwd values/RPC wire sequences, rawLine JSON matching message objects, exact final-result equality to attempt 2, successful cleanup and workspace deletion. Thus neither P1 affected these particular four records, and their unchanged hashes preserve that conclusion.

Each real attempt has initialize protocolVersion number 1 and session/new error code -32603, message Internal error, data.details Request failed with status code 500. No exact sessionId, real prompt, prompt RPC id, update stream, prompt terminal or successful isolated write exists. Both generated B IDs were never sent. Zero workspace delta before any prompt is not a successful read-only prompt proof. The current error is not a prompt-error terminal. Host's separate successful session/new observation does not replace these records; these four failures do not prove a permanent server blocker or any authentication state.

## Verification

Reviewer cwd: `E:/wx_lifeilin/github.com/lifei6671/serena-desktop`.
TASK=`.trellis/tasks/09-26-cb5-003-fresh-session-prompt-activity-contract`.

- Independently executed `python -B TASK/test_run_probe.py`: PASS, exit 0, 5 tests, no real subprocesses. Tests write only owned disposable temp fixtures.
- Current Rust evidence retained: 7 unit + 7 integration = 14/14 PASS; cargo fmt PASS. Rust unchanged since these actual runs; no redundant Rust rerun in this repair review.
- Target hashes: PASS, 85/85. Real evidence continuity: PASS, 10/10. Unchanged Rust/package hashes: PASS, 5/5.
- Independently checked original product baseline: PASS, 319 files, delta 0.
- Parent scope evidence: git diff --check PASS, exit 0; HEAD unchanged; tracked changes empty. User .zed and unrelated tasks preserved.
- Earlier cargo check/clippy passed before UUIDv7 changes; no new claim of those commands running after the feature change. Current Rust tests compile the changed package. No configured Python lint/typecheck was introduced; Python regression execution validates the repaired runner.
- Real CLI after repair: NOT_RUN by design; per-scenario retry allowance already exhausted. Linux, Job-at-creation and process-tree containment: NOT_PROVEN/outside scope.

Stop and wait for Host Gate. No authenticate, credential/token access, product changes, CB5-004, or Git commit occurred in this review.
