# CB6-002 Verification

## Environment

- cwd: `E:\wx_lifeilin\github.com\lifei6671\serena-desktop\src-tauri`
- host: native Windows PowerShell
- Rust tool: `C:\Users\lifei\.cargo\bin\cargo.exe`
- Trellis CLI: `UNAVAILABLE` — `py` exists but reports `No installed Python found`; task lifecycle/artifacts were maintained manually in the repository format.
- CodeGraph: `UNAVAILABLE` — connector required approval while host approval policy is `never`; exact local reads and `rg` were used.
- Node: `UNAVAILABLE` — no `node.exe` on PATH or under `C:\Program Files\nodejs` / `C:\Program Files (x86)\nodejs`; no dependency was installed or downloaded.
- Real CodeBuddy/CB5 probe: `NOT_RUN` by scope.
- Linux/Docker/macOS: `NOT_RUN`; this delivery is Windows-only and does not claim cross-platform evidence.

## Required verification

| Check | Result | Evidence |
|---|---|---|
| `cargo fmt --all -- --check` | PASS | exit 0 |
| `cargo test --lib agent::codebuddy::windows_launcher::tests -- --nocapture` | PASS | 8 passed, 0 failed, 1304 filtered |
| `cargo test --lib agent::codex::windows_launcher::tests -- --nocapture` | PASS | 3 passed, 0 failed, 1309 filtered |
| `cargo test --lib agent::codex::runtime::tests -- --nocapture` | PASS | 17 passed, 0 failed, 1295 filtered |
| `cargo check --lib` | PASS | exit 0 |
| `cargo clippy --lib -- -D warnings` | PASS | exit 0 |
| `cargo clippy --lib --tests -- -D warnings` | FAIL (pre-existing) | only `src/agent/store/usage_tests.rs:987` `clippy::await_holding_lock`; await sites 1012/1016 |
| `cargo clippy --lib --tests -- -D warnings -A clippy::await_holding_lock` | PASS | exit 0; scoped substitute proves no other lib/test Clippy finding |
| `git diff --check` | PASS | exit 0 |

MSVC emitted its normal import-library linker stdout warning during focused tests; all listed passing commands exited 0.

## Contract evidence

- The real Windows fixture observes membership as its first application operation; the launcher uses `PROC_THREAD_ATTRIBUTE_JOB_LIST`, so the primary process is assigned before its initial thread becomes runnable.
- The fixture launches a native descendant and observes `DESCENDANT:1:1`, proving inherited child-tree containment without shell or download.
- An optional real Node + local script branch is present and would prove Node root plus native descendant membership, but the current host has no Node; real-Node evidence is honestly `UNAVAILABLE`.
- Stdio handle list, non-inherited Job/extra event, KILL_ON_CLOSE, explicit `TerminateJobObject`, invalid policy, inheritable-Job rejection, acquisition cleanup, and post-create `CreatedChild` ownership all pass in the isolated fixture.
- Pure tests cover verbatim local projection, ordinary/verbatim UNC projection, unsupported UNC fail-closed, identity mismatch/missing directory, Host env inheritance, duplicate PATH replacement, Unicode/special `=C:` environment entries, double-NUL block, diagnostics redaction, wrapper/relative-script rejection, and CB6-001 LaunchSpec wiring without ACP.
- Codex source hashes and focused launcher/runtime regressions remain unchanged/passing; no Codex semantic or evidence boundary was modified.

## Trellis quality check

- mode: `trellis-check`, full CB6-002 scope
- result: PASS
- changes made by checker: none
- findings: no P0/P1/P2
- freshness: four delivery-owned hashes matched the final pre-freeze state
- Material Contract Difference: none
