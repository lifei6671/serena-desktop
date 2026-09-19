# P3-003 Technical Design

## Authority and semantic projection

`executions` remains the current snapshot authority. A store-private semantic projection reads
the persisted lifecycle (`status`, `dispatch_state`) and current activity pair, maps lifecycle to
`ProgressPhase`, then calls the already-frozen `derive_summary_code`. Its tuple is:

```text
(activity_phase, tool_category, activity_summary_code)
```

For a Provider event the pair comes from the allowlisted event; for lifecycle work the pair stays
persisted. This preserves Finalizing/Reconciling priority without clearing underlying activity.

`derive_activity_revision` belongs to the activity domain and computes the fixed v2 SHA-256
canonical JSON array. It excludes timestamps, age, silence, heartbeat, `updated_at`, Usage, and
sequence, so identical tuples always receive the same revision.

## Transaction design

The existing StateStore connection serialization and `BEGIN IMMEDIATE` transaction are the sole
concurrency mechanism. Activity projection runs after fail-closed validation and before commit.

- Equal tuple: execute only a monotonic `last_activity_at` update. It never writes `revision` or
  `updated_at`.
- Changed tuple: checked `activity_sequence + 1`; update current values and timestamp; INSERT the
  same sequence into `execution_activity_events` with v2 revision and the semantic-change time.
  Any error returns from the existing transaction closure, rolling both writes back.
- Lifecycle CAS: calculate and append any overridden-summary semantic event before/after the
  existing lifecycle UPDATE within its transaction. The lifecycle UPDATE remains the sole
  `revision=revision+1` write; an attached Activity update does not add another increment.

## History query design

A store-internal request accepts `execution_id`, `after_sequence: Option<i64>`, and a requested
limit. Negative cursors are rejected; requested limit is capped at a private maximum of 100. The
SQL always orders `sequence ASC` and fetches `limit + 1`; the extra row establishes the exclusive
next cursor. Returned events carry sequence, pair, summary, revision, and observed time only.

## Test design

Focused transaction tests use real Store transactions and deterministic synchronization barriers
for the concurrent activity/lifecycle case. They assert DB current/history/revision/timestamps,
not only API success. Existing tests that encoded obsolete Activity revision-CAS behavior are
changed to the frozen P3-003 contract. Migration tests remain regression evidence only.

## Resolved contract correction — nullable post-override summary

The original v8 DDL declared `execution_activity_events.summary_code TEXT NOT NULL`, while the
P3-003 contract requires a lifecycle semantic event when leaving Finalizing/Reconciling. A valid
execution can have no underlying Activity pair: its current summary changes from
`execution.finalizing` or `execution.reconciling` to `null`.

Host authorization corrects the uncommitted v8 contract to nullable `summary_code`. This is the
only minimal representation consistent with `derive_summary_code`, the v2 JSON-null hash input,
and mandatory semantic history: no history omission, sentinel, generic code, v9 migration, or
second transaction is introduced.
