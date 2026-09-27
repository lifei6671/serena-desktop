# Delivery review context

Scope: only CB2-004 delivery-owned frontend changes and tests, plus this task's artifacts. Baseline frontend files were clean. All previously dirty backend paths and CB2-002/003 artifacts are excluded from change ownership but may be read for contract context. baseline.json records HEAD, initial status and hashes of all existing dirty/untracked files.

Authority: user approved CB2-004 implementation and task creation/planning, forbids UI/role dropdown, Rust changes, Remote MCP changes, CB2-005, commits and task-unrelated cleanup. Host independent gate is explicitly separate from this internal review.

Contracts: versioned task and design hashes in prd.md; CB-002 defaults, fixed five roles, open provider IDs, §26 exclusive local mutation. Current Rust config.rs is the wire source; Supervisor.replace_config preserves authoritative agent_providers under operation lock. Frontend must retain policy on round trips and use returned policy, not manufacture migration defaults over existing snapshots.

Risk depth: Tier 2, focused on configuration compatibility and React state synchronization. Full-scope read-only independent child review after verification and target freeze. TypeScript and JavaScript review profiles, configuration/API surfaces, correctness/state/concurrency/testing/scope lenses from code-delivery-review apply. New code/core logic needs Chinese comments. Frontend specs are currently placeholders; no UI implementation is authorized.

Required evidence: behavioral config/controller tests for unknown IDs/null values, partial save, snapshot/draft preservation, initial defaults and fixtures; npm test (at least related tests), npm run build, scoped lint/diff check. Windows frontend execution is valid evidence for this task; no claim of Linux or native runtime validation. Existing Rust Clippy await_holding_lock blocker is excluded and untouched.

Inventory and frozen file hashes will be recorded in target.json after implementation. Verification evidence belongs to verification.md. Reviewer must inspect actual diff and untracked tests, confirm complete coverage and matching hashes, report findings with severity/path/evidence or No findings, and terminal PASSED/BLOCKED/UNAVAILABLE. Do not modify implementation during the read-only delivery review; report any fixes to the coordinator.
