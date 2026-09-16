# Implementation Plan — P2A2-004

1. Inspect existing v6 migrations, frozen identity, request hash, Work transaction, and focused test conventions; retain the supplied source hashes as pre-change reference.
2. Add transactional v7 migration and store registration/tests, including migration matrix and raw SQL checks.
3. Propagate generation across execution/work-run persistence and validate nonzero input plus triple consistency.
4. Implement v2 hashing with narrowly guarded legacy v1/pre-C2 compatibility and regression coverage.
5. Propagate the new field at snapshot construction sites without changing Authority; add Begin/Start/Continue behavior tests.
6. Run requested targeted tests, scoped rustfmt check, locked library check, diff checks, then a read-only full-scope delivery review. Do not commit or push.
