# Implementation Plan — P2A3-004 Capacity Accounting 与 LRU Eviction

1. Add only in-memory Manager scheduling metadata: live-capacity accounting, deterministic usage sequence, and acquire reservation state.
2. Route Runtime acquire through the Manager boundary. On full capacity, atomically detach one eligible LRU handle into `Stopping`, then call Provider stop outside the mutex.
3. On stop success atomically admit the replacement; on failure restore the returned handle and fail without starting a replacement.
4. Add deterministic fake-provider tests for LRU success/order, busy/no-victim behavior, failure retention, and Stopping/acquire race.
5. Run the targeted Rust test module, formatting check, Cargo check, and diff check; then freeze and review the delivery-owned diff without committing.
