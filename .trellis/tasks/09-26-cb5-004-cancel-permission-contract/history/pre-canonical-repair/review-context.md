# CB5-004 delivery review context

Authority: current user request and existing prd/design/implement; scope is only this task and disposable workspaces. Task was activated with task.py start. No CB5-005, product modifications, commits, user settings edits, credential reads/logs, or unknown-side-effect replay.

Baseline: HEAD 314687f9ec0ab8bb6115971cf1edc6e8ef116b2d; tracked clean; 755 tracked hashes recorded in evidence/scope-baseline.json. Pre-existing other tasks and .zed are excluded and must be preserved. The six initial task planning files are user-supplied context; task.json planning-to-in_progress is the authorized lifecycle change.

Risk tier: Tier 3 protocol/concurrency/permission probe, bounded to task-local code. Review full Rust/Python executable inventory, Cargo dependency/lock provenance, deterministic tests, sanitized real wire, manifests, child ownership/cleanup, exact identity and ordering, and result classifications. Do not infer provider terminal from cancel intent, deny, EOF, or child exit.

Required verification: all task-local Rust tests; real cancel-before, cancel-after, permission-deny (explicit PARTIAL if genuine requests cannot be triggered); cargo fmt check; whole-tree git diff check; final HEAD and tracked content unchanged. Native Windows evidence is not Linux evidence. No WSL. SDK must be official agent-client-protocol 2.2.0 external streams.

Review strategy: FULL_SCOPE, independent CHILD_AGENT, read-only target inspection. Reviewer must not alter implementation. Return exact reviewed target hash, coverage, evidence-backed findings and PASSED/BLOCKED/UNAVAILABLE. Read code-delivery-review Rust/Python profiles and relevant protocol/config/test lenses. Main session will repair through implementer, reverify affected behavior, refreeze, and re-review if needed (at most three repair rounds).

Spec-sync decision: this task freezes observations in task-local evidence for Host Gate. Shared specs and primary designs remain unchanged under the explicit no-product-change/task-only boundary. Host Gate remains distinct from independent task-local review.
