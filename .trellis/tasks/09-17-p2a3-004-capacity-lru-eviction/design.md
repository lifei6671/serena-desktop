# Design — P2A3-004 Capacity Accounting 与 LRU Eviction

## Boundary

Only `workspace_capability.rs` Manager/Slot internals and its fake-provider tests change. Idle timeout, background sweep, Remove and shutdown remain outside this task.

## Linearization and ownership

The Manager-owned slot-map mutex is the scheduling boundary. Under it, an acquire reserves its Slot before awaiting a permit; a capacity eviction selects one eligible `Ready` victim, changes it to `Stopping`, and transfers its sole handle to the stop operation. The Provider call happens after the mutex is released.

The Manager tracks a monotonically increasing in-memory usage sequence. Each admitted acquire receives a distinct sequence, so the smallest sequence is the deterministic LRU victim without relying on `HashMap` iteration or wall-clock resolution. Pending acquires make a Slot ineligible until they either obtain a guard or cancel, eliminating the handle-transfer race while retaining the existing `in_flight` meaning for returned guards.

## Failure behavior

Provider stop success with `Stopped` evidence atomically marks the victim stopped and admits the original replacement request. A `CapabilityStopFailure` returns the moved handle to the same Slot, restores `Ready`, retains capacity, and returns the existing safe stop-failed error. No replacement start is scheduled on this path.

## Exclusions

No idle timer/sweeper, general stop single-flight, Remove/shutdown integration, provider-specific logic, DTO change, persistence, dependency change, commit, or push.
