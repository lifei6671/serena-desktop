# P3-002 Implementation Plan

1. Add audited v8 SQL: DDL, temporary migration validation trigger, deterministic
   CASE backfill, then trigger removal.
2. Wire `SCHEMA_V8` into the single existing transaction and project current
   fields through `ExecutionRecord`.
3. Add focused migration tests for fresh v8, v7 backfill, constraints, foreign
   keys, rollback, and all old-version regression targets.
4. Run the required focused tests and static checks; then perform a read-only
   full-scope delivery review of P3-002-owned paths only.
