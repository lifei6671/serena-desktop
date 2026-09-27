# Independent FULL_SCOPE review — round 1

Reviewer: CHILD_AGENT /root/full_scope_review, did not implement or modify target. Read-only; no validation commands executed by reviewer.
Target: 31517182DC296FC89C2917C4282E1282FD8F46190B5B36199F27300CEAA2ABE7; baseline c890919ff8cfedab6f73682f257ff1cc889d9027. All 8 file hashes and target hash matched.
Gate returned: BLOCKED. One P2; no P0/P1. Main accepted the finding for scoped repair round 1.

## Finding R1 — final drain submission order

prompt.rs:187–191 repeatedly polls publish with now_or_never. ExecutionTelemetryProjector submits Store spawn_blocking work via transactions.rs:366; dropping a pending future does not cancel its submitted transaction. A final batch tool_call(Read) then tool_call_update(Completed) can therefore submit concurrent writes. If Read obtains the mutex after Provider, final category incorrectly remains tool.read. Existing in-flight publication can also overlap final-drain writes. project_activity_semantics semantic change path (transactions.rs:793 onward) does not reject reordered events.

Slow pure-pending test does not cover irreversible submission; gated projector test waits before terminal. Reviewer requests no multiple pending final submissions or conservative drop, plus deterministic submitted-work/reordering regression. No public sink/schema changes required.

## Coverage

Full text and tracked diff/untracked content reviewed:
- src-tauri/src/agent/codebuddy/activity.rs
- src-tauri/src/agent/codebuddy/activity/tests.rs
- src-tauri/src/agent/codebuddy/mod.rs
- src-tauri/src/agent/codebuddy/prompt.rs
- src-tauri/src/agent/codebuddy/prompt/activity_tests.rs
- src-tauri/src/agent/codebuddy/prompt/tests.rs
- src-tauri/tests/fixtures/codebuddy_activity_host.jsonl
- src-tauri/tests/fixtures/codebuddy_prompt_child.rs

Reviewer also inspected public telemetry/sink/projector, Store writes, protocol drain, provider capabilities, SDK v1 and task authority. Remaining classification/identity/privacy/bounds/private authority/capability behavior found consistent; SDK has no Material Contract Difference. Windows evidence acknowledged; historical failed checks and unavailable Linux/Host probe remain as documented in verification.md.
