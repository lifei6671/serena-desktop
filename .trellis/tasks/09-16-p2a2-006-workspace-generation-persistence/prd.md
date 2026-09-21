# P2A2-006 Execution Workspace Generation Persistence / Migration Compatibility

## Goal

Validate the already implemented frozen `workspace_generation` persistence, schema migration, and request-key compatibility. This task owns no Workspace Authority routing or snapshot construction.

## Requirements

- Verify schema v7 checked, nonzero `workspace_generation` columns on `executions` and `work_runs`, including atomic historical upgrade to generation `1` without rewriting existing identity, hash, claim, runtime, or evidence facts.
- Verify a new Execution record persists and reads generation exactly; validate generation `0` rejection and existing persisted identity consistency checks.
- Verify old-database upgrade, restart, fixtures, and narrowly guarded legacy request-key compatibility. When generation participates in the frozen persisted hash contract, validate the applicable version and compatibility guard.
- For historical nonterminal Executions, accept only migration from already persisted authoritative facts; when no fact can establish generation, remain fail-closed rather than inferring from a selected, session, or requested workspace.
- Treat `14bec7a` as the implementation/evidence baseline for the v7 migration, generation persistence, v2 hash, and focused tests. Do not reintroduce or expand legacy routing.
- Do not alter public MCP DTOs, WorkspaceResolver semantics, Source/Agent schemas, recovery/runtime/capability contracts, dependencies, frontend, commits, or pushes.

## Acceptance Criteria

- [ ] A fresh database ends at `user_version=7`; v6 data migrates with generation `1`, schema defaults/checks work, migration remains atomic/idempotent, and version `8` is rejected without rewriting it.
- [ ] Execution and WorkRun generation are stored and returned exactly; zero is rejected and persisted identity consistency remains enforced.
- [ ] v2 hashes distinguish generations; exact legacy retries remain accepted only under their frozen persisted-identity guards, without rewriting historical hashes or dispatching a provider.
- [ ] Historical nonterminal migration uses only authoritative persisted facts and fails closed when generation cannot be proved.
- [ ] Focused migration, persistence, request-key compatibility, restart, fixture, and regression tests pass along with scoped formatting, locked library check, and diff checks.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
