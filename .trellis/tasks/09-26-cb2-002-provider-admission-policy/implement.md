# CB2-002 Implementation Plan

1. Add the disabled Provider error classification and a provider/control admission helper.
2. Wire current CB2-001 settings into `AgentTaskManager` and Product startup without adding a mutation surface.
3. Apply admission to start/dispatch, continue preflight, and resume pending; preserve registration-only cancel and startup reconcile.
4. Add focused tests for the frozen routing matrix, state preservation, running drain behavior, and Codex regression.
5. Run formatting, focused tests, relevant compile/lint checks, inspect the final diff, and complete a delivery review.

## Rollback Points

- If settings cannot be consumed without a new authority, stop with `DESIGN_BLOCKER`.
- If admission would need to mutate registry health or weaken Claim/recovery semantics, stop with `DESIGN_BLOCKER`.

## Non-Goals

- CB2-003 Local IPC and policy mutation commands.
- Remote MCP contract changes.
- UI or CodeBuddy Runtime/ACP implementation.
- Git commit.
