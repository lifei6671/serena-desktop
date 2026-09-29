# Provider catalog narrow regression fixes design

## Boundaries

- Change only the existing CodeBuddy ACK reconciliation, AgentPanel catalog request bookkeeping/layout, catalog IPC lock scope, and obsolete ExecutionProfile tests.
- Treat the current dirty worktree as the authoritative implementation baseline. Existing hunks outside the five repairs remain user-owned and must not be reverted or reformatted.
- Preserve the current typed execution profile and provider admission/catalog APIs.

## Behavior

1. CodeBuddy ACK handling inspects acknowledged `configOptions`. When the acknowledged option category is `mode`, it mirrors the selected option into the legacy modes response exactly as before the regression. Existing model then reasoning sequencing is untouched.
2. AgentPanel tracks successful catalog keys separately from currently executing keys. A request inserts into in-flight before invocation, removes from in-flight in `finally`, and only adds to success cache after resolution. Polling can therefore retry failures, coalesces concurrent requests, and skips already successful keys.
3. The IPC handler acquires Broker management authority only long enough to resolve the canonical workspace identity and clone a stable TaskManager handle. Provider admission and catalog I/O execute through TaskManager after the management guard is dropped.
4. The responsive CSS media rule explicitly places the label in column 1 and each selectable/status row in column 2. Desktop declarations remain unchanged.
5. ExecutionProfile production code is unchanged. Tests assert canonical sparse serialization for typed fields and deserialization failure for unknown fields.

## Verification and rollback

- Run the exact targeted commands requested by the user plus `git diff --check`.
- Freeze and independently review the delivery-owned hunks after validation.
- Rollback is deletion/reversion of only this task's hunks and task record; no schema or data rollback is involved.
