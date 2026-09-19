# P2D Batch C Implementation Plan

1. Define exact camelCase frontend Health DTO types and local IPC methods.
2. Replace duplicated ProjectPanel capability status with descriptor-driven Health UI and bounded action/event lifecycle.
3. Extend DOM tests for every required health/action state, including third-provider rendering and event cleanup.
4. Audit the existing Remove/Shutdown implementation against frozen semantics and add focused deterministic Rust integration tests; only repair a demonstrated P0/P1 gap.
5. Run focused tests and the requested formatter, check, typecheck/build and regression commands; perform final delivery review without commit/push.
