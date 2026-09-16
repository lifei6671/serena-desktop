# P2A1-013 Registry Core Gate

## Goal

Close the Phase 2A.1 Registry main-path gate with focused migration, restart,
and concurrent CRUD evidence. This task validates the committed Registry and
Project Management implementation; it does not begin Phase 2A.2 routing.

## Requirements

- Verify the current `ManagerConfig.workspaces` Registry behavior for migrated
  configurations, persisted CRUD mutations, restart restore, and concurrent
  readers/writers.
- Verify the P2A1 main path: `workspace_register`, rename/reorder, remove
  claim guard, DesktopSelectedWorkspace, and the absence of startup Serena
  Registry overwrite.
- Preserve the existing explicit additive Serena import behavior as optional:
  if covered, it must be additive and idempotent; it must not become a startup
  authority path.
- Add only focused integration coverage or minimal fixes required by a failing
  P2A1 gate assertion. Do not change WorkspaceLease, MCP request authority,
  Source/Git routing, capability runtime, provider behavior, schemas outside
  the established P2A1 Registry boundary, dependencies, commits, or pushes.

## Acceptance Criteria

- [ ] Migration preserves legacy workspace identity and the Registry is the
  sole persisted authority after restart.
- [ ] Register, rename/reorder, selection, and remove behaviors persist
  atomically; remove retains the filesystem and rejects an active claim.
- [ ] Startup does not overwrite the Registry from Serena; explicit import,
  where exercised, is additive and idempotent.
- [ ] Concurrent readers observe complete Registry snapshots only.
- [ ] Targeted Rust and applicable ProjectPanel tests pass, together with
  `cargo check --locked --lib`, scoped formatting, diff checks, and a final
  delivery review.

## Goal

Verify and close the P2A1 Registry migration, restart, and concurrency integration gate without entering P2A2 request routing.

## Requirements

- TBD

## Acceptance Criteria

- [ ] TBD

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
