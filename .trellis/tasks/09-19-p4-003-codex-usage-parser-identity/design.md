# P4-003 Design

## Boundary

`agent/codex/protocol.rs` owns pinned wire decoding. `agent/codex/provider.rs` owns private Thread/Turn identity validation and maps only an already-bound notification to the Provider-Agnostic event. `agent/provider/telemetry.rs` remains identity-agnostic and owns the safe event value type.

## Data flow

```text
thread/tokenUsage/updated
  -> Codex private UsageNotification { thread_id, turn_id, total, last, context }
  -> existing envelope/root/runtime checks
  -> exact persisted execution binding
  -> UsageEvent { execution, provider, cumulative snapshot, observed_at }
  -> AgentEventSink
```

The parsed `last` snapshot stops at the Codex-private notification. No parser or adapter state is introduced, so duplicate and out-of-order valid totals remain observable and P4-004 retains sole ownership of delta/regression semantics.

## Error semantics

Recognized Usage shape failures are mapped to `USAGE_EVENT_INVALID` by using the P4-001 validation error code. Identity failures are not numeric/schema failures and follow the existing Activity observability drop path.

## Exclusions

No schema/store projection, baseline, epoch, delta, completeness, revision, terminal grace, Product, MCP, or UI behavior is added.
