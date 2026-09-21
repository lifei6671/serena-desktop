# P4-002 Usage Database Migration

## Goal

Implement schema v9, ordered atomic v8-to-v9 migration, and minimal private/public usage store read scaffolding with migration and constraint tests. Excludes parser, delta, lifecycle, Product, MCP, UI, commits, and push.

## Requirements

- Authority is the host-verified technical design §37–§38 and task card P4-002,
  plus the accepted Usage Contract DCR.
- Add schema v9 only. It creates `execution_usage`,
  `codex_thread_usage_epochs`, and `codex_execution_usage_state`; it must not
  alter the Activity or lifecycle schema.
- Register `SCHEMA_V9` and migrate supported empty v0 or versions 1 through 8
  in order to v9 inside the existing one transaction.
- Historical executions receive no `execution_usage` row. Absence represents
  null counters and `unknown`, with no fabricated zero or Codex-private row.
- Provide only minimal table records and read helpers. The public read record
  maps losslessly to P4-001 `UsageSnapshot`; private records remain within the
  store module and are not Product/public DTOs.
- Test fresh and real-v8 upgrades, restart preservation, every required FK / PK
  / CHECK, absence of legacy checkpoints, and rollback when v9 fails.

## Acceptance Criteria

- [x] A fresh database opens at `user_version = 9`; a real v8 fixture upgrades
  atomically and an existing execution has zero usage rows.
- [x] The three v9 tables and every specified constraint are present; obsolete
  `codex_thread_usage_checkpoints` is absent.
- [x] Read scaffolding retains null-versus-zero and public/private boundaries
  without writing or computing Usage.
- [x] The focused migration/store tests, locked cargo check, format check, and
  diff check pass.
- [x] No parser, delta/revision update, terminal lifecycle mutation, Product,
  MCP, UI, commit, or push is introduced.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
