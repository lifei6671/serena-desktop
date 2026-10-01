# CB6-005 independent full review context

Baseline: feat/codebuddy, 0d96524599189b541bbcb4fc9a961ee2456f8a19, clean. Entire current delivery-owned source/test delta is in scope, including untracked files. No commit/push. Strict independent CHILD_AGENT / FULL_SCOPE / read-only required by user. Reviewer must not have implemented reviewed changes. Final executable-target.json will identify frozen files and hashes; verification.md and recovery-matrix.md provide actual evidence.

Authority: current user frozen request (full inherited transcript), task prd/design/implement, docs implementation-task-breakdown CB6-005 and technical-design §6.3/§23/§31/CB-006 Gate, CB6-002 launcher and CB6-004 private state. Root AGENTS.md requires Chinese function/core comments. Applicable code-delivery-review references: project-rules, change-scope, review-protocol, verification, severity, change-surfaces (persistent data), review-lenses, languages/rust.md.

Risk: Tier 3 persistent ownership, FFI handle lifecycle and Claim release. Check each sealed evidence construction site and complete-write transaction; identity/policy/session/name preconditions must precede OpenJobObjectW. No PID/reap/exit evidence. Only exact zero or verified ERROR_FILE_NOT_FOUND. QUERY|TERMINATE, non-inheritable returned handle, live policy recheck, bounded poll; failures retain Claim and do not mint proof.

Check CodeBuddy provider ownership, generic execution R1 and immutable private R1 consistency; no missing-private-state session fabrication or Claim release. Missing private state versus generic-only Job stopping must be explicitly tested/documented. Evidence commit failure must leave unknown and retain Claim. Complete/idempotent reads must reject other provider evidence. Orphans covered.

Check only CodeBuddy durable work selected by its startup adapter, and CodeBuddy rows cannot enter Codex recovery. Existing generic ClaimRecovery outcomes and generic finalization remain authoritative; no Provider ID release transaction special case, no result recovery/R2/Codex Thread/Turn invocation. Startup per-item failure maps existing generic kinds; store/global failure is Err, preserving TaskManager health contract.

Check actual TaskManager build/refresh, disabled settings and unavailable discovery paths retain recovery authority and execute registered recovery without CLI. Only canRecover may become true after evidence gate; other five capabilities and availableForNewExecution remain false. No schema/migration/Usage/UI/MCP/public kind additions.

Native Windows validation only; do not claim Linux results. Baseline cargo clippy --lib --tests -- -D warnings reproduced exit 101 solely usage_tests.rs:987 await_holding_lock (await 1012/1016), task/baseline-clippy.log. User specifically requests precise baseline recording; this is not global lint PASS.

Reviewer returns exact target identity, all-file + requirement coverage, concrete evidence-backed P0/P1/P2 findings with line locations, remaining verification gaps, and PASSED/BLOCKED/UNAVAILABLE. No source edits or mutating checks during review.
