# CB6-002 Implementation Plan

1. Freeze baseline hashes and record the dirty-worktree exclusions, tool limitations, design contracts, and review plan.
2. Add Windows-only `codebuddy::windows_launcher` wiring with private verified path/environment/LaunchSpec request types and explicit post-create ownership transfer.
3. Add pure and isolated Windows tests, including a controllable descendant process fixture and an optional local-Node branch with no network/download.
4. Run focused CodeBuddy launcher tests, focused Codex launcher/runtime regressions, formatting, compile/check, clippy, and scoped diff checks. Record exact PASS/FAIL/NOT_RUN/UNAVAILABLE evidence.
5. Freeze the delivery-owned target using per-file SHA-256 plus an aggregate SHA-256. Verify scope and baseline preservation.
6. Dispatch an isolated read-only reviewer with the frozen target and complete context. Repair any P0/P1/P2 finding, rerun affected checks, refreeze, and re-review within the three-round limit.

## Verification boundary

- Native Windows evidence proves only this launcher/path/environment contract; it does not prove ACP, CodeBuddy product behavior, Host Gate, Linux, Docker, or macOS.
- If Node is absent, the real-Node assertion is `UNAVAILABLE` and the equivalent native child-tree fixture is the containment evidence.
- The known unrelated Clippy `await_holding_lock` finding remains out of scope if still present; report the first material error exactly.
- No real CodeBuddy binary, CB5 probe, external service, install, commit, or push is permitted.
