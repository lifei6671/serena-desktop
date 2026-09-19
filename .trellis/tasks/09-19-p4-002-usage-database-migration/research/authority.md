# P4-002 Authority Snapshot

- `docs/technical-design-agent-platform-v0.2.md` §37–§38, SHA-256
  `42c0f86bb294dc0425c5a8deee5d9f3f36586704218a832e8a9fe89e5f1d9f19`:
  the exact v9 public and Codex-private table shapes, absence semantics, and
  private/public boundary.
- `docs/implementation-task-breakdown-agent-platform-v0.2-revision003.md`
  P4-002, SHA-256
  `d86aedbe60ac8701fbcbff06369566d33f282454c6eef5f4aae198db1908132a`:
  scope, atomic rollback, no historical zero backfill, and required test
  categories.
- Host verification pins `schema_v8.sql`, `store.rs`, and `usage.rs` as the
  direct implementation baseline. Their SHA-256 values were rechecked before
  implementation and match the supplied values.

The accepted Usage Contract DCR permits generic public `complete`; it does not
authorize any P4-003 parser or P4-004 update/lifecycle work here.
