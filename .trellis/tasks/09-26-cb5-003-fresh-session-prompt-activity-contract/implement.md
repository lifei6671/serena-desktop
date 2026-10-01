# CB5-003 Implementation Plan

1. Capture current product/hash baseline and reuse/copy only the necessary CB5-002 task-local SDK harness pattern.
2. Inspect official SDK 2.2.0 session/new, prompt, update/client-handler APIs; do not guess JSON fields.
3. Build task-local harness that owns CodeBuddy Code child and can service/capture server notifications/client requests required during prompt.
4. Implement recursive temp-workspace manifest + SHA256 verification.
5. Run read-only fresh-session scenario without conversationRequestId.
6. Run second fresh-session isolated write scenario, with conversationRequestId and `--permission-mode auto` if required for local temp tool execution.
7. Capture exact prompt terminal response and all session/update ordering.
8. Safely probe terminal/error behavior if deterministic; otherwise record NOT_PROVEN.
9. Run fake peer/unit tests for session id missing/malformed, update-before-session, unexpected terminal shapes, timeout/EOF cleanup.
10. Produce evidence files and independent task-local review.
11. Verify src/src-tauri product hashes unchanged, whole-tree git diff --check PASS.
12. Stop before CB5-004.

## Rollback

If session/new or prompt cannot be proven on the real CLI, return BLOCKED/PARTIAL and keep `canExecute=false`. Never compensate by assuming SDK schema implies provider behavior.