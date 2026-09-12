# Runtime Attempt / Quarantine design decision

Current attempt identity: legacy runtime-{executionId} guesses miss randomly named
pre-bind Runtime attempts. Runtime::create_blocking persists preparing before Job /
CreateProcess; Execution binding follows connect/initialize and is too late.

Chosen identity: schema v4 execution_runtime_attempts(execution_id, runtime_instance_id,
created_at). Explicit reservation follows compatibility verification and precedes Runtime::create.
Runtime IDs stay independent and warm reuse does not reserve/create another Runtime.
A single store helper queries the mapping plus legacy origin-ID rows. Runtime reference
has no FK because reservation must commit before the Runtime row exists. Execution FK,
unique Runtime identity, append-only triggers keep durable facts immutable. Existing
v1/v2/v3 schemas are preserved; migration is additive.

Create boundary: under exclusive workspace lease, validate the pending Execution and
Claim, then verify the binary and commit attempt reservation before Runtime::create. Mapping-only crash is conservatively
attempted too. Runtime-row/initialize/pre-bind crash cannot become PendingExplicitResume.
Guard, snapshot and failed-provider logic share the attempt predicate. No replay.
Recovery: unbound attempted Execution becomes unknown/manual resolution, Claim retained;
Runtime recovery independently uses mapping workspace identity and existing Job evidence.
Backward compatibility: legacy origin-ID lookup retained in the same authority helper;
existing bound Runtime rows supply workspace association. Unattributed historical random
Runtime rows cannot be assigned to a workspace by guessing; conservative global quarantine
is used only for these identity-less legacy rows when recovery cannot prove termination.

Quarantine: Ready entries retain ManagedClient as before. Retained failures carry store,
workspace, runtimeId, error and optional owner; presence means Quarantined, even when
client slot is vacant. One lease gate rejects AGENT_RUNTIME_QUARANTINED. Admission also
uses that predicate before dispatch receipt (including resume_pending). Independent
workspaces continue. Retry is explicit, serialized with workspace lease; no execution
or turn is replayed. Shutdown stops admission, waits workers, drains Ready entries and
reconciles retained failures. Owned failures call terminate; ownerless failures query DB
then recover named Job using verified policy. Quarantine clears only for terminated +
complete + job_active_processes_zero/managed_job_destroyed + timestamp. No PID/EOF/Drop
inference. DB errors keep quarantine. No TTL, UI, Observe v2 or protocol change.

Verification: required pre-bind arbitrary ID/startup and reservation-only crash, legacy
attempt, quarantine no launch/isolation, explicit recovery/cold continue, ownerless second
shutdown, warm 4-turn regression; targeted/full cargo tests, check, Clippy, diff, independent
review covering all eight user questions. Material Contract Difference: none in current
API; missing historical workspace association is handled fail-closed, never invented.

Compatibility validation does not constitute a managed workspace Runtime attempt:
its probes use isolated temporary cwd and no durable Execution side effects. Preserve
existing BACKEND_UNAVAILABLE/canResumePending behavior when verification fails before
reservation. After verification, both original and recovery connect commit the matching
reservation before Runtime::create. The deterministic connector seam reserves at the
same creation boundary. No attempt record is deleted to manufacture replay eligibility.

Startup must reconcile/retain all orphan recovery Runtime rows before any new recovery
connection. Repeated claimed recovery retries the Pool's retained owner, never opens a
second owner. Quarantine records are unique by workspace/runtimeId and stay present
across awaited convergence, including ownerless worker failures.
