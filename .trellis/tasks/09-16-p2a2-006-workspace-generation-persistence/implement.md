# Implementation Plan — P2A2-006 Execution Workspace Generation Persistence / Migration Compatibility

1. Inspect the `14bec7a` v7 migration, record persistence, hash compatibility, and focused test baseline; confirm no task-owned source drift.
2. Verify transactional v7 migration, raw SQL constraints, old-database upgrade, restart, and fixture coverage.
3. Verify nonzero generation persistence/readback and the frozen persisted-identity compatibility guards.
4. Verify v2 hash participation and narrowly guarded historical compatibility without rewriting stored hashes or dispatching a provider.
5. Record that Begin/Start snapshot construction belongs only to P2A2-007 and Continue inheritance only to P2A2-008; do not edit either route here.
6. Run targeted migration/persistence/compatibility tests, scoped formatting, locked library check, and diff checks. Do not commit or push.
