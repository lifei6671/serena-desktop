# P3-002 Activity Schema Migration

## Goal

Upgrade the Agent state database from schema v7 to v8, adding the Activity v2
current-summary fields and a bounded Activity history table without implementing
Activity mutation, history queries, Observe/MCP/UI exposure, pruning, or changes
to `executions.revision`.

## Requirements

- Add v8 DDL for `activity_summary_code`, non-negative `activity_sequence`, and
  `execution_activity_events` with `(execution_id, sequence)` primary key and
  `ON DELETE RESTRICT` parent reference.
- Preserve all supported v1..v7 data and finish a fresh or upgraded database at
  `PRAGMA user_version = 8`.
- Backfill only the current summary deterministically from persisted
  `status`/`dispatch_state` and Activity columns using the P3-001 mapping.
- Historical `activity_sequence` starts at zero and no historical history rows
  are invented.
- Invalid non-overridden historical Provider/category or Tool/None combinations
  fail the v8 transaction closed, without advancing `user_version` or leaving
  v8 DDL behind.
- `ExecutionRecord` may read the two new current fields only as necessary for
  migration coverage.

## Acceptance Criteria

- [ ] New databases expose v8 columns, table, check, key, and foreign-key
      constraints.
- [ ] v7 and each supported historical version reach v8 while retaining legacy
      execution/runtime/work/claim data.
- [ ] Backfill covers finalizing, reconciling, Provider, all Tool categories,
      no Activity, and priority overrides.
- [ ] Invalid historical Activity combinations abort atomically and leave the
      old schema version intact.
- [ ] Focused migration tests, P3-001 Activity tests, locked Cargo check,
      format check, and diff check pass.

## Out of Scope

- P3-003 Activity semantic updates, history query/cursor APIs, history pruning,
  Product DTO/Observe/MCP/UI exposure, execution revision changes, commit, and
  push.
