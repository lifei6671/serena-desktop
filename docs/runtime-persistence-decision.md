# Persistent Codex Runtime — design decision

Date: 2026-09-12. Baseline: `06d1fbc7757913b08596ede56eaf3a8febadb28c`;
initial index/worktree clean. Observe v2 in this checkout is preserved.

## Audit (before implementation)

| Boundary | Current evidence / decision |
| --- | --- |
| Current Runtime Ownership | `codex/provider.rs::execute_with_acceptance` creates `runtime-{executionId}` on every dispatch; `run_managed` unconditionally shuts it down. |
| ManagedClient Ownership | `app_server/managed.rs::connect` transfers Runtime to an independent reconciliation monitor before initialize. ManagedClient owns Client and the monitor join handle. Shutdown consumes ManagedClient; RuntimeFailure retains the Job owner on failed convergence. Preserve this chain. |
| Client Execution-specific State | `title_scope`, immutable observability Root/Turn, activity watch slot, buffered lifecycle events and pending RPCs need an execution boundary. Interrupt futures, dispatch/terminal flags and turn identity are Provider-stack local. No current child-thread tracking collection exists. RequestGuard cancels transport on abandoned RPC. |
| Runtime DB Contract | `schema_v1.sql` has a nonunique executions_runtime_state index and Runtime foreign keys; no uniqueness across Execution.runtime_instance_id. The immutable Runtime binding trigger remains. No schema change. |
| Workspace Claim Contract | Workspace claim is unique/exclusive and finalization atomically releases it. Pool lease must additionally serialize Client access through that release/return interval. |
| Release Evidence Contract | `coordinator.rs::finish` validates exact Runtime/Thread/Turn/result/cleanup identities, commits SameRuntimeCleanup, result and terminal, releases Claim while Runtime is still alive. Termination is only necessary for uncertain/error convergence. No safety check needs removal. |
| Host Shutdown Path | `lib.rs::request_exit` waits for `commands::shutdown_impl` and prevents exit on error. Add awaited Agent shutdown here. Existing path has no Agent hook. |
| Startup Recovery Path | `task_manager/recovery.rs::recover_startup` currently selects Claims only. Add old-host nonterminated Runtime rows without Claims; reuse `runtime::recover`, which validates Session/named Job/policy and observes active_processes_zero or managed_job_destroyed. Missing policy/access errors remain unknown, never fabricated termination. Single-instance application startup establishes prior Host exit. |
| Title Scope / Activity Binding | Bind title to each durable execution; clear title/observability/activity only after safe completion, with no unresolved requests. Ignore already completed Turn events by exact identity, never attribute them to the next execution. Do not clear loaded Threads. |
| AppServer Thread Loading Semantics | Fixed source cached under TASK-005 history-recovery-verification/source/thread_processor.rs has one ThreadManager, get_thread/remove_thread and loaded-thread resume handling. Client supports thread/start and exact thread/resume. Initial audit required real second-turn wire verification; the final pinned-binary run below confirmed it. |

## Ownership and flow

AgentTaskManager clones share an Arc<CodexRuntimePool>. Entries are keyed by canonical
workspace root; each entry has an exclusive asynchronous lease and owns at most one
ManagedClient. The ManagedClient stays in the entry throughout execution. Runtime IDs
are independent host-generated instance IDs. Client retains explicitly loaded Thread
metadata and completed Turn identities. This is serial workspace execution, not multiplexing.

Fresh: pool miss -> connect/initialize -> thread/start -> turn/start. Warm: pool hit
and requested Thread loaded -> direct turn/start. Cold/unloaded: connect if needed ->
thread/resume exact durable Thread -> turn/start. Each turn has a new durable Execution.

Only authoritative terminal + complete result + SameRuntimeCleanup + released Claim
and a healthy, settled Client permit reuse. Failed model turns may be safely terminal;
protocol/transport/cleanup uncertainty invalidates the entry. An unresolved start ACK
does not revoke safe terminal evidence but prevents Runtime reuse. Never replay a turn.

Shutdown first closes admission and signals active providers, waits for owned dispatch
workers (including reconciliation), then drains idle entries and awaits every monitor.
Failed shutdown retains Runtime ownership/evidence diagnostics for an explicit retry;
application exit remains blocked on incomplete evidence. Drop is only a crash backstop.

Startup also reconciles old-host orphan Runtime rows even when all their executions
are complete. Cold continuation remains new Runtime + thread/resume. No TTL, UI,
schema, permission/networkAccess, title rule or Observe v2 redesign.

## Material Contract Difference

Audit: none requiring weakened Execution safety or a schema migration. Missing Host
shutdown/orphan selection and Client scope cleanup are in the explicitly authorized scope.
Real App Server warm-turn semantics: PASS with pinned 0.153.4 binary. Evidence:
`tasks/evidence/runtime-persistence/real-product-45740-1789188305610/` (final run).
The same Runtime and Thread handled two distinct Turns with one initialize, one
thread/start and zero thread/resume. A subsequent fresh lineage/cancel also reused
that Runtime and shutdown waited for Job evidence. Human Desktop UI remains unverified.

An intermediate real run also passed the warm pair, then the separate fresh cancellation
case sent interrupt after the start ACK but before turn/started. Codex rejected it with
`no active turn to interrupt`; Provider correctly entered reconciliation and retained
its Claim. The ignored active-cancellation test now waits for authoritative `running`
and Turn identity. No production cancellation/retry policy changed. Failed wire is
retained at `real-product-21872-1789187980705`; final full E2E passed.

## Review refinements

Legacy `runtime-{executionId}` rows still participate in existing pending-attempt
fail-closed checks. New Runtime IDs have no Execution-derived component.

Recovery failures transfer ownership before any subsequent fallible Execution write.
A missing owner alone is not evidence of a pre-create error: the generated Runtime ID
and durable row decide whether a failed connection left an unconfirmed Runtime.
Every shutdown failure after obtaining ManagedClient, including monitor JoinError,
is retained by the Pool and prevents successful application exit. Owned failed Jobs
can be retried by shutdown; ownerless failures without conclusive evidence remain
blocked rather than being silently dropped.

Late name notifications drained at the execution boundary update both loaded-root
metadata and the durable thread_names projection. Warm dispatch does not overwrite
the projection from an older cached Thread response.

## Acceptance and review

Required: warm 2/4 turns (one launch, one initialize, one start, zero resume), cold resume,
failure invalidation/no replay, released Claim with live Runtime, two-workspace shutdown,
orphan convergence, title/activity/terminal isolation. Preserve ignored fixed-binary E2E
and capture raw wire counts. Run user-requested repository checks and independent review
answering all eleven ownership/lifecycle questions. Human Acceptance = PENDING.
