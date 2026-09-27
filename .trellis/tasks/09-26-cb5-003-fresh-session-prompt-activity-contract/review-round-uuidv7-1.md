# Independent review — UUIDv7 correction, round 1

Reviewer: CHILD_AGENT `/root/review_probe`, Strict, Tier 3. Read-only target review; no implementation changes or real CLI executions.

Target: `17aae46ce9433f045ff0b808b9833d5d0ddd519f6ec0a59b789c00d7c97ad11e`.
Coverage COMPLETE for the affected changes and their integration with previously reviewed unchanged source. All 77 target hashes and 30 pre-UUIDv7 archive hashes match. Historical pre-login coverage remains valid. Cargo.lock, transport.rs and contracts.rs are unchanged; reviewed changes cover Cargo.toml, main.rs, run_probe.py, task PRD/design and all refreshed evidence/verification.

Review gate: **BLOCKED**. Delivery verdict: **CHANGES_REQUESTED** for the two defects below. Independently, real contract acceptance remains BLOCKED / UNABLE_TO_VERIFY because no real session/prompt succeeds.

## Findings (fixed)

None; coordinator owns repairs.

## Findings (not fixed)

### [P1] Require a fresh result file for each retry

`run_probe.py:30`, `run_probe.py:61`

Confidence: High. Both attempts write/read the same scenario result path. If attempt 1 writes a session/new HTTP500 result and attempt 2 exits without writing evidence, `output.exists()` still succeeds. The old result is read, its harnessExitCode changed, and its attempt changed to 2 before archiving. This falsely represents old PID/cwd/wire as a fresh attempt.

Deterministic read-only mock reproduced: attempt 1 wrote the existing failed fixture; attempt 2 exited 1 without writing a file. Generated attempt-2 record had the same PID, cwd and wire as attempt 1. No real CLI was started and target files were not modified.

Minimal repair: use a new output path per attempt (and reject preexisting results), require that attempt's output, and preserve no-evidence failures without reusing prior evidence. Add a focused Python regression.

### [P1] Do not retry or report success after an earlier cleanup failure

`run_probe.py:81`, `run_probe.py` final return over `reports`

Confidence: High. Retry eligibility checks session/new HTTP500 only, ignoring harness exit, cleanup and workspace deletion. Final success considers `reports` (the selected last result per scenario) instead of every attempt. An initial unreaped child can therefore be followed by another process and disappear from the overall success decision.

Deterministic read-only mock reproduced: first report had cleanup.succeeded=false, directChildReaped=false, errors=[final_wait: timeout], harnessExitCode=1; later scenario finals claimed success. The reporter launched subsequent attempts and returned 0. This violates the required cleanup gate even though the per-attempt file records the failure.

Minimal repair: stop before retry when process cleanup/deletion or harness execution fails; require all attempts to satisfy cleanup and deletion for aggregate success. Add a focused Python regression for the failed-first/successful-later case.

## Verified unaffected facts

- UUID generation is now `Uuid::now_v7().simple().to_string()` with uuid v7 feature. Test asserts exact length 32, lowercase hex, no hyphen, parse version 7, exact metadata key/value, and A absence. Cargo.lock has no byte change.
- Sanitization and reporter distinguish PromptResponse.result._meta, session/update.params._meta, nested update._meta and public providerData correlations. Current evidence correctly marks all echo locations NOT_PROVEN because no prompt was sent.
- Actual four attempts are fresh: PIDs 4760, 45188, 48504, 3928; four distinct temp cwd values and distinct RPC ids. Each has 4 actual frames; all 16 rawLine values parse to recorded message objects. Each per-attempt wire exactly matches its combined-wire slice. Final read/write result bytes equal their respective attempt-2 files.
- Actual four cleanup records all succeed; workspaces are deleted. Thus the two demonstrated error-path defects do not invalidate these particular recorded four attempts.
- Four initialize results negotiate number 1. All four session/new results contain exactly code -32603, message Internal error, data.details Request failed with status code 500. No sessionId/prompt/update/terminal was fabricated.
- Both generated B identifiers parse as UUIDv7, are 32 lowercase hexadecimal characters, and were not sent. Required/optional/accepted/echoed/correlation behavior remains NOT_PROVEN.
- Updated verification treats 500 only as this execution's observation, acknowledges Host's independent session/new success, and makes no permanent-failure or authentication-state inference.

## Verification provenance and limits

- Current Rust log: 7 unit + 7 integration = 14/14 PASS. Current fmt record/parent command result: PASS. These checks do not exercise the Python retry orchestration defects.
- Prior unchanged transport review and bounded-cleanup tests retained. No broad checks or real CLI reruns performed by reviewer.
- Two focused in-memory Python subprocess mocks executed against current reporter; both defects reproduced. Mock data was written only into owned disposable Windows temp directories, removed by TemporaryDirectory cleanup.
- Current scope evidence reports HEAD 5179248cfa30096df912c6567aee2e80e3bf4352, 319 product hashes with delta 0, tracked changes empty, diff check exit 0.
- No authenticate, token access, product edits, CB5-004, Git commit, or Linux validation.
