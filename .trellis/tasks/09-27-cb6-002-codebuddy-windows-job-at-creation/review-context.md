# CB6-002 Final Review Context

## Mode and depth

- strict independent read-only review required by the user
- risk: Tier 3 / Windows process ownership, unsafe FFI, external path identity, environment and resource lifecycle
- strategy: one full-scope code partition; review context includes CB6-001 input and Codex regression boundary

## Frozen requirements map

| Requirement | Implementation | Verification |
|---|---|---|
| first-runnable Job ownership | `JOB_LIST` in `windows_launcher.rs` before `CreateProcessW` | real fixture first observation + root membership |
| KILL_ON_CLOSE/no breakaway/non-inherit | Job setup plus `validate_job_policy` | policy flags, inheritable negative test, close/terminate tests |
| explicit stdio whitelist | `HANDLE_LIST` with exactly three child pipe ends | inherited Job/event probes both fail |
| resolved real executable | `LaunchRequest::from_resolved` and strict `.exe/.com`/Node args validation | wrapper, relative script and wrapper script rejection |
| external path authority | `ExternalProcessPath::verify` using config canonicalize/identity helpers | local, UNC policy, mismatch and missing path tests |
| full Host env plus refreshed PATH | `ProviderChildEnvironment` full owned block | inheritance, PATH dedupe, Unicode, `=C:`, double NUL, redaction tests |
| post-create ownership | handles adopted immediately; `LaunchError.created` | injected post-create failure returns five owned handles |
| Codex non-regression | no Codex/Cargo/config/discovery edits | preserved hashes + 3 launcher and 17 Runtime tests |

## Review scope

- `src-tauri/src/agent/codebuddy/mod.rs`: only the Windows launcher module wiring hunk.
- `src-tauri/src/agent/codebuddy/windows_launcher.rs`: full file.
- `src-tauri/src/agent/codebuddy/windows_launcher/tests.rs`: full file.
- `src-tauri/tests/fixtures/codebuddy_launcher_child.rs`: full file.

Review neighboring CB6-001 `ResolvedLaunchSpec`, config identity helpers, and Codex launcher/runtime only to validate contracts and regressions; they are not delivery-owned changes.

## Exclusions and limitations

- All other dirty files/tasks are prior user/task work and excluded.
- Node is absent, so optional real-Node execution is `UNAVAILABLE`; equivalent native child-tree containment passes.
- Strict tests Clippy is blocked only by the known pre-existing `usage_tests.rs:987 await_holding_lock`; scoped substitute passes.
- No ACP, real CodeBuddy, persistence/recovery, Claim, UI, remote, Linux, Docker or macOS conclusion is claimed.
- CodeGraph and Trellis Python CLI are unavailable for the recorded environment reasons.

## Reviewer response contract

Return review mode, exact target identity, complete coverage statement, findings by P0/P1/P2 with file:line evidence, `PASSED|BLOCKED|UNAVAILABLE`, remaining verification risks, and freshness. Do not edit files or run real CodeBuddy/CB5/ACP.
