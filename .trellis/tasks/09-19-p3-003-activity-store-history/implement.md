# P3-003 Implementation Plan

1. Inspect the existing Activity projection, lifecycle transaction, revision helper conventions,
   public Store surface, and focused test fixtures.
2. Add the pure Activity Revision v2 helper and Store-private progress/semantic projection.
3. Replace Activity's legacy revision-CAS update with atomic heartbeat/semantic paths; attach
   lifecycle-driven semantic projection to `transition_execution` without changing lifecycle rules.
4. Add the internal bounded history query types/API and its SQL implementation.
5. Add focused deterministic tests for semantic/heartbeat/revision/hash, lifecycle override,
   concurrency, rejection/rollback, pagination, and migration regressions.
6. Run the user-required focused Rust tests and static checks using the approved Docker runner
   when Linux validation is required; then perform a full-scope, read-only delivery review of
   P3-003-owned changes only.
