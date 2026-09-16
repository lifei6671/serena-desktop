# P2A2-005 Local Tauri IPC Workspace Authority Foundation / Audit

## Goal

Audit the Local Tauri Workspace authority boundary, lock the explicit
`workspaceId` DTO/serialization contract that is independently safe today,
and map each IPC surface to its atomic Authority cutover owner.

## Requirements

- The Desktop may use its selection only to prefill a new-task form. The
  outgoing IPC request must carry its final explicit `workspaceId`.
- The Local Agent Start DTO serializes the final explicit `workspaceId`.
- Desktop selection remains a frontend default only; it must not be treated as
  a Rust Command fallback.
- Each unready backend surface is mapped to its atomic cutover owner without
  changing its public behavior, Root source, Execution snapshot, Claim, or
  Provider Dispatch.

## Acceptance Criteria

- [x] The audit maps Agent Start to P2A2-007, Git to P2A2-009, CodeGraph to
  P2A2-010 / the later Workspace Capability Adapter, Serena Semantic to
  P2A3-010, and Source to P2A3-011.
- [x] The Local Agent Start DTO serializes an explicit `workspaceId`; Desktop
  selection remains a frontend default only.
- [x] No unready backend route gains validation-only behavior, an Execution
  snapshot, a Claim mutation, or a Provider Dispatch change.

## Resolved boundary decision

Local Agent Start's Authority cutover is intentionally moved to P2A2-007. That
later task must atomically implement:

```text
request.workspaceId
  -> WorkspaceResolver
  -> WorkspaceLease
  -> freeze workspace_id / canonical_workspace_root / workspace_generation
  -> create Execution / Claim transaction
  -> Provider Dispatch
```

P2A2-005 must not validate an Agent Start ID without making that ID determine
the actual snapshot Root. It only records and tests the independently safe
Local IPC boundary until P2A2-007 applies this atomic route.

## Verification evidence

- `node --test src/agentRequests.test.mjs`: PASS — 2 passed; the Local Agent
  Start DTO serializes `workspaceId` and excludes caller-provided Root fields.
- `node --test src/AgentPanel.test.mjs`: PASS — 53 passed; the existing retry
  regression preserves the original explicit workspace ID across a UI Workspace
  switch, confirming selection is only a new-form default.
- `npm run build`: PASS — TypeScript and production Vite build completed.
- Audit found no direct Local Tauri execution path for Git, CodeGraph, Serena
  Semantic, or Source; their owner mapping is recorded in `design.md`.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
