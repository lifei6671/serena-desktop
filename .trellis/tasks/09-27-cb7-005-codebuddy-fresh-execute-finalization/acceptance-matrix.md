# Acceptance evidence map

All rows require implementation and verification evidence before PASS. Initial status: NOT_RUN.

| Requirement | Scope / required evidence |
| --- | --- |
| Public execute and frozen identity | Registered resolved launch only; Execution provider/workspace/current lifecycle mismatch rejected; no runtime on invalid context |
| Exact send boundary | MarkSent commit -> acceptance -> Dispatching commit -> exact SDK request full write + inner.flush -> Dispatched -> Running; response/timer cannot substitute |
| Send failures/drop | Private Sent->Uncertain; generic Dispatching->Uncertain; no replay; no terminal fabricated; owned Runtime cleanup |
| Staging | OCC/runtime/provider/status checked; safe public result + provider terminal atomic; Finalizing with Claim retained and no release evidence; real Job alive checkpoint |
| Generic release | Existing Reconciling->Interrupted; staged Finalizing->exact terminal only; approved original runtime evidence; staged result/completeness immutable |
| Atomic rollback | Fault at result/release/Claim delete rolls back entire transaction; same durable evidence retry exactly once |
| Evidence failure | Job open/query/policy/timeout and evidence persistence faults -> Unknown + Claim; PID disappearance never sufficient |
| Startup window | Staged exact provider/private terminal retained through recovery; same generic finalizer; incomplete/conflicting identity fail closed |
| Native write slice | Real CreateProcessW+Job-at-creation+pipes+SQLite; isolated cwd; only output.txt expected bytes; complete result/evidence/Claim deletion |
| Native read slice | Same public execute, zero workspace delta |
| Policy/health | found+enabled+available+canExecute admits; missing/unavailable/disabled blocks new only; accepted lifecycle/recovery survives policy change |
| Activity | Exact identity safe category; slow/failure has no terminal/release authority |
| Regressions | Full CodeBuddy parallel evidence + serial if flaky; generic state/finalization, product/TaskManager admission/catalog, telemetry, Codex same-runtime/Usage |
| Tooling | fmt/check; clippy baseline precisely attributed; diff/scope/frozen hashes; independent read-only FULL_SCOPE review |
| Capability release gate | Only after above scoped gates PASS: Windows execute/activity/recover true, others false; non-Windows fresh/activity/recovery false |

No real CodeBuddy probe. No commit/push. Linux runner unavailable; no WSL and no Linux PASS claim.
