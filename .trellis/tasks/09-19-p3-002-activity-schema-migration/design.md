# P3-002 Technical Design

## Authority and compatibility

`executions` remains the current Activity authority. v8 adds its current
summary and sequence fields plus an empty-on-migration history relation. The
existing `ExecutionRecord` is extended only to project those stored values.
History `summary_code` is nullable: null is the exact absence of a derivable
summary and is never replaced with a sentinel or generic code.

The SQL backfill freezes the already-persisted Product projection:

- `dispatch_pending/not_dispatched` => Pending;
- `dispatch_pending/dispatching` => Dispatching;
- `dispatch_pending/dispatched`, `running`, `cancel_requested`, and
  `cancelling` => Running;
- `finalizing` => Finalizing;
- `dispatch_pending/uncertain`, `reconciling`, and `unknown` => Reconciling;
- completed/failed/cancelled/interrupted => Terminal.

Finalizing and Reconciling branches come before all Activity branches. The
remaining SQL CASE exactly encodes P3-001: Provider/None, the six Tool
categories, and no Activity.

## Failure and transaction design

The v8 SQL creates a temporary-in-lifetime validation trigger before the
backfill UPDATE. It aborts non-overridden malformed Activity pairs with
`AGENT_ACTIVITY_CONTRACT_ERROR`; the trigger is dropped only after a successful
backfill. `migrate` already wraps every schema step and `user_version` update in
one IMMEDIATE transaction, so a trigger abort rolls back columns, table,
trigger, data, and version together.

No history INSERT occurs in v8. `activity_sequence` uses the schema default of
zero for every historical row. No retention or cascade behavior is introduced.

## Test design

Tests create a real v7 schema and rows, then verify the summary matrix,
sequence, empty history, constraints, fail-closed rollback, and repeated open.
Existing v1..v7 regression expectations update only their final schema version
while preserving their row-retention assertions.
