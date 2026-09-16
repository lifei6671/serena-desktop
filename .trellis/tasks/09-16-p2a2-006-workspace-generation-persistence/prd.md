# P2A2-006 Execution Workspace Generation Migration

## Goal

Persist frozen workspace generation for Work and Execution identity with schema v7 and request-key compatibility.

## Requirements

- Add schema v7 with checked, nonzero `workspace_generation` columns on `executions` and `work_runs`; migrate historical rows to `1` atomically without touching existing identity, hash, claim, runtime, or evidence fields.
- Freeze and persist `{workspace_id, canonical_workspace_root, workspace_generation}` for Work and Execution creation, retrieval, and continuation.
- Reject generation `0` using the existing invalid-request style; require all production creation paths to pass a generation explicitly.
- Upgrade new request-key hashes to the frozen v2 tuple including generation while accepting exact legacy v1 and pre-C2 continuation hashes only under the specified identity guards.
- Preserve current legacy ActiveWorkspace Authority for Work/Agent routing. Do not alter public MCP DTOs, WorkspaceResolver semantics, Source/Agent schemas, recovery/runtime/capability contracts, dependencies, frontend, commits, or pushes.

## Acceptance Criteria

- [ ] A fresh database ends at `user_version=7`; v6 data migrates with generation `1`, schema defaults/checks work, migration remains atomic/idempotent, and version `8` is rejected without rewriting it.
- [ ] Execution and WorkRun generation are stored and returned exactly; zero is rejected and Work/Execution triples must match.
- [ ] v2 hashes distinguish generations; exact legacy v1 and pre-C2 retries remain accepted only when compatible, without changing stored historical hashes or dispatching a provider.
- [ ] Begin, Start, and Continue preserve the same frozen workspace triple; mismatches fail as `WORKSPACE_CONTEXT_MISMATCH` before side effects.
- [ ] Focused migration, identity, request-key, Work, and regression tests pass along with scoped formatting, locked library check, and diff checks.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
