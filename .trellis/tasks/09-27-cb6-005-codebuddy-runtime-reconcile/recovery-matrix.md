# CB6-005 Recovery Matrix

Native Windows fake Job / child evidence only. No real CodeBuddy CLI, CB5 probe, ACP session/load, result recovery, session/new, prompt, or CB7 execution was run.

| Condition | Test | Expected authority/result |
| --- | --- | --- |
| Native Job already empty | `native_zero_destroyed_and_idempotence` | exact ActiveProcesses=0 → durable `job_active_processes_zero` |
| Verified name/session/policy, missing native Job | same | only ERROR_FILE_NOT_FOUND → durable `managed_job_destroyed` |
| Native parent exited, descendant active | `native_tree_main_pid_gone_still_requires_job_zero` | PID exit is diagnostic; original Job remains active until TerminateJobObject + exact zero |
| Wrong session/name/policy/provider | `invalid_identity_retains_claim` | no complete evidence; ExecutionUnknown or pre-existing generic ExecutionInconsistent; Claim retained |
| Invalid runtime id `\\`, `/`, NUL/empty | `invalid_runtime_ids_never_form_evidence` | rejected before opening Job |
| Session query, AccessDenied, other Open error, accounting query, policy query/policy, terminate error, timeout | `os_failure_matrix_keeps_unknown_and_claim` | cfg(test) observer faults at exact Win32 boundaries; Runtime unknown, ExecutionUnknown, Claim retained |
| Persisted policy correct, native live policy incorrect | `native_live_policy_mismatch_is_unknown` | native policy observation rejects evidence |
| Evidence write failure | `durable_evidence_commit_failure_and_snapshot_conflict` | actual SQLite trigger abort; Runtime unknown; no complete evidence/release |
| Ownership snapshot changes before evidence commit | same | Store transaction rejects sealed proof from old owner snapshot |
| Complete same-provider row | `native_zero_destroyed_and_idempotence` | idempotent success without modifying evidence timestamp |
| Complete other-provider row | `other_provider_complete_is_rejected` | rejected before consuming evidence |
| Missing private state | `private_missing_and_conflict_fail_closed` | generic original Job can safely stop, but ExecutionUnknown + Claim retained; no Session invented |
| Existing private R1 conflict | same | no OS mutation or complete evidence; Claim retained |
| Missing binding / actual missing Runtime row | `missing_runtime_retains_claim_and_orphan_is_recovered`, `missing_runtime_row_is_unknown` | ExecutionUnknown + Claim retained |
| Orphan with valid/invalid identity | `missing_runtime_retains_claim_and_orphan_is_recovered`, `orphan_unknown_and_codex_isolation` | OrphanResourceRecovered / OrphanResourceUnknown |
| Pending explicit resume / inconsistent / already released | `generic_claim_classification_is_preserved`, `already_released_claim_keeps_generic_outcome` | existing generic ClaimRecovery semantics unchanged |
| Host reopen + disabled CodeBuddy + missing CLI + refresh | `disabled_missing_cli_refresh_retains_recovery_authority` | real Store close/reopen and Manager/Registry startup → interrupted + release, health remains unavailable |
| Provider partition | `orphan_unknown_and_codex_isolation`, final Codex regressions | Codex startup skips CodeBuddy rows; CodeBuddy scans only codebuddy rows |

## Authority boundaries

- `TerminationEvidence` fields are private to CodeBuddy recovery; only the validated Win32 observer can construct production evidence. PID/token never enter the observer.
- Store rechecks provider, owner, Job name/session/policy/platform/containment and generic/private R1 bindings during completion. It does not update Codex usage/private tables.
- Missing private state is separated from existing conflicting state. No private Session/Prompt identity is synthesized. Missing private never permits Claim release.
- Post-termination interrupted/release uses existing `Finalization { basis: RuntimeTerminated, completeness: Unknown, result: None }` transaction. No Provider ID branch was added to release authority.
- Capability Gate is native Windows only; non-Windows recovery remains fail-closed. macOS Codex changes are limited to provider-scoped selection and have no runtime validation in this task.
- Fault-injection cases are deterministic boundary tests, not claims of native Windows AccessDenied/timeout occurrence. Native zero/destroyed/policy/process-tree cases use real Win32 Jobs.
