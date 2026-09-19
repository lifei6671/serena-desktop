# P4-004 Design

## Boundary

`store::usage` owns private epoch/state persistence and public snapshot projection. The Codex provider selects a baseline intent only after its exact Thread bind, then performs best-effort late-turn invalidation before it can publish Usage. `ExecutionTelemetryProjector` gates only on its public execution ID and delegates the event unchanged to the Store.

## Safety rules

- The only subtractable input is Codex provider total tokens within the same `(runtime, thread)` epoch.
- Fresh zero is written only when its absence proof is transactional. Warm observation reads only the current epoch. Every other case is unknown.
- Store errors and late-event handling remain telemetry degradation; Execution/Claim/lifecycle control paths do not consume them.
- No terminal telemetry-state transitions are introduced in this task.
