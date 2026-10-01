# CB7-005 implementation verification

Status: implementation and scoped capability/catalog regressions complete; ready for freeze and independent read-only FULL_SCOPE review. Known unrelated baseline failures remain explicitly excluded below. No commit/push, real CodeBuddy probe, schema migration, Cancel/Continue/Usage implementation.

## Execution environment

Windows native Rust toolchain C:/Users/lifei/.cargo/bin/cargo.exe; command-local PATH prepends C:/Users/lifei/.cargo/bin and C:/nvm4w/nodejs. Exact cwd/commands/exit/log paths are in validation-results.jsonl. Root cwd commands explicitly use --manifest-path src-tauri/Cargo.toml; commands recorded from src-tauri use its manifest implicitly. Counts overlap; no summing. Linux UNAVAILABLE: no project Docker runner; no WSL validation. Tool output linker stdout warnings are separate from exit status.

## Implemented authority boundaries

- SDK retains request-id generation and waiter completion. A single exact session/conversation session/prompt observer captures the SDK frame id and emits only after all bytes and inner.flush succeed. The observer never routes a response. MarkSent -> accepted -> durable Dispatching -> exact physical flush -> Dispatched -> Running precedes terminal handling.
- ProviderTerminalResult atomically stores terminal/runtime evidence and safe result/completeness in existing Execution columns, enters Finalizing, and retains Claim. JSON null represents a staged empty public result; SQL NULL continues to mean no staged result. None and Some(JSON null) compare as the same public JSON value.
- RuntimeTerminated keeps Reconciling -> Interrupted recovery and adds Finalizing -> the exact staged terminal. Same original Runtime/provider plus approved durable termination evidence remain release authority. Staged result/completeness cannot be replaced by finalization input. SameRuntimeCleanup branch semantics are unchanged.
- Startup claim classification preserves valid staged Finalizing. Unknown recovery may enter Reconciling then typed ResumeStagedTerminal only with staged terminal and approved same-Runtime evidence; CodeBuddy additionally verifies exact private terminal identity/stop reason. Missing/corrupt private evidence never trusts result JSON alone.
- One execute owner holds preparation/prompt/shutdown/finalization through caller drop. Its R1 id is frozen before preparation; losing direct-execute contenders cannot clean up another attempt. Runtime shutdown return is not release authority; missing evidence becomes Unknown with Claim retained, without a second cleanup retry in execute.
- Windows execute/activity/recover are enabled only after scoped Fresh/atomic behavior gates passed; cancel/continue/token_usage stay false. Non-Windows capabilities stay false. Registered missing CLI descriptors/capabilities remain queryable while admission stays unavailable.

## Acceptance evidence map

| Boundary | Current test evidence |
| --- | --- |
| Public native write/read-only + exact bytes/delta | native_public_execute_write_and_readonly_atomic_release: real CreateProcessW, Job-at-creation, SDK pipes, SQLite; only output.txt = CB7_005_WRITE\n for write, zero Workspace files for read-only. Control and compiled peer are outside Workspace. |
| Exact ordering | Native acceptance sink independently reads SQLite Sent/not_dispatched; child requires acceptance and committed dispatching/dispatched before receiving Prompt; SQLite trace verifies Dispatching < Dispatched < Running < Finalizing; physical observer tests prove flush boundary. |
| Exact SDK id / no fake completion | sdk_prompt_flush_id_matches_wire_without_completing_waiter; prompt_flush_observation_is_exact_and_physical; write_and_flush_closed_pipe_keep_first_failure_and_health_local. |
| Job alive staged crash window | native_staged_live_job_startup_preserves_terminal_and_result asserts real QueryInformationJobObject ActiveProcesses > 0 and Claim retained, then startup from missing-CLI registered skeleton stops original Job and preserves exact terminal/result. |
| Evidence persistence failure | native_execute_eof_and_evidence_persistence_failure asserts Unknown + Claim with staged Completed/result retained; later explicit startup with new successful evidence preserves Completed. |
| Pre-flush failure / caller drop | native_post_accept_preflush_failure_is_uncertain_then_interrupted closes real child input before session/new reply while keeping stdout live; accepted, Dispatching -> Uncertain, no received Prompt, private Uncertain, approved termination -> Interrupted. native_caller_drop_after_flush_converges_without_replay covers post-flush drop. |
| No terminal | native_execute_eof_and_evidence_persistence_failure: EOF with complete Runtime evidence only yields Interrupted and unknown completeness. |
| Concurrent owners / disable | native_competing_execute_only_cleans_its_own_attempt proves exactly one Runtime/winner; native_disable_after_acceptance_preserves_owned_execute uses Registry/admission and disables shared policy in accepted callback while current execution completes. |
| Atomic rollback / mismatch | staged_finalization_faults_rollback_and_same_evidence_retry injects result/release/Claim-delete failures, verifies full rollback then exactly-once convergence; staged_terminal_result_requires_original_runtime_evidence_and_exact_values checks all four terminals, None/null/text, mismatched result/completeness/status/provider. |
| Generic recovery graph | exact_11_by_11_matrix and status_edges_execute_and_forbidden_edges_reject include the new guarded restore edge; missing staged result or termination evidence is rejected. |
| Job failures and descendants | Existing CB6-005 os_failure_matrix_keeps_unknown_and_claim, native_live_policy_mismatch_is_unknown, native_tree_main_pid_gone_still_requires_job_zero, durable_evidence_commit_failure_and_snapshot_conflict, private_missing_and_conflict_fail_closed pass in full CodeBuddy. |
| Activity | Native slow telemetry never completes; safe public category assertion and CB7-004 full activity/projector failure tests pass without changing results or finalization. |
| Product/admission | found/missing, enabled/disabled catalog matrix plus actual registered admission; TaskManager missing-CLI bypass stays unaccepted, no Runtime attempt. |

## Failure history and baseline attribution

- native-initial.log: 0/2 FAIL because test LaunchSpec replaced --acp with a custom argv. Production frozen launcher correctly rejected it before process creation. Fixture corrected to locate external control through its own executable directory; production validation unchanged.
- codebuddy-parallel.log: 115 PASS / 1 FAIL, newly introduced missing Runtime row handling. Fixed approved-evidence read to preserve prior Unknown + Claim behavior. gate-codebuddy.log 117/117 PASS; regate-codebuddy.log 120/120 PASS includes additional ownership/SDK tests. No failure silently skipped.
- gate-agent-store-transactions-tests.log: 50 PASS / 2 FAIL due prior 23-edge literal fixture; required new evidence-guarded restore edge added to matrix and transaction setup. regate-agent-store-transactions-tests.log 52/52 PASS.
- task-manager-full.log: 47 PASS / 1 FAIL / 1 ignored. authority_state_failure_uses_stable_internal_code at recovery/tests.rs:207 expected Err but received Ok([]). This exact failure is documented at baseline in CB7-004 verification.md/task-manager.log and CB7-003 baseline-task-manager.log; unchanged recovery fixture. Do not describe the full suite as passed.
- clippy-initial.log: FAIL exit101, only usage_tests.rs:987 await_holding_lock, awaits 1012/1016. Frozen baseline source unchanged; not repaired or suppressed.
- Existing ignored real-Codex tests remain ignored; they are not passing runtime/Host evidence. No real CodeBuddy or CB5 probing was performed.

## Final validation results

All commands, actual cwd and exits are in validation-results.jsonl. PASS counts below are test bodies, not filtered/ignored tests; overlapping filters are not summed.

| Scope / final log | Result |
| --- | --- |
| Generic transactions / regate-agent-store-transactions-tests.log | PASS 52/52, exit 0 |
| Full CodeBuddy serial / final-serial-codebuddy.log | PASS 120/120, exit 0; includes all 7 new native Execute tests |
| CodeBuddy public provider / verified-agent-codebuddy-provider-tests.log | PASS 5/5, exit 0 |
| Catalog snapshot / verified-agent-product-provider_catalog_tests.log | PASS 5/5, exit 0 |
| MCP provider queries / verified-mcp-providers.log | PASS 4/4, exit 0 |
| Admission / final-serial-agent-provider-control.log | PASS 1/1, exit 0 |
| TaskManager ordinary tests / gate-agent-task_manager-tests.log | PASS 30/30, exit 0; narrower than full TaskManager |
| Full TaskManager / task-manager-full.log | FAIL 47 passed / 1 baseline failed / 1 ignored, exit 101 |
| Telemetry / gate-telemetry.log | PASS 11/11, exit 0 |
| Same Runtime / gate-same_runtime.log | PASS 2/2, exit 0 |
| Usage / gate-usage.log | PASS 57/57, exit 0 |
| Full Codex / final-serial-agent-codex.log | FAIL 149 passed / 1 baseline failed / 6 ignored, exit 101 |
| fmt / verified-fmt.log | PASS exit 0 |
| check / final-check.log | PASS exit 0 |
| Clippy / verified-clippy.log | FAIL exit 101, only unchanged usage_tests.rs:987 await_holding_lock, awaits 1012/1016 |
| Diff whitespace / verified-diff-check.log | PASS exit 0 |

Additional retained attempts:

- final-codebuddy.log parallel: 117 passed / 3 failed. Two obsolete capability assertions were updated to the approved Windows gates. The pre-flush fixture had only closed Win32 standard input while the CRT still owned a readable handle; corrected fake child closes CRT fd 0 and any distinct Win32 handle, checking close returns, before replying session/new. Production flush semantics were not weakened. The final full serial 120/120 regression includes this deterministic native failure case.
- final-serial-agent-product-provider_catalog_tests.log: 4 passed / 1 failed because the shared JSON snapshot still advertised execute/activity=false. Updated only CodeBuddy's two capability values; final catalog and real MCP transport tests above pass.
- final-clippy.log had two new assertions_on_constants plus the baseline Usage lint. Replaced the constant assertions with a runtime admission outcome comparison; verified-clippy.log now has exactly the one baseline error.
- final-serial-agent-codex.log fails only trait_startup_reconcile_projects_shared_recovery_errors_to_operation_failed at adapter_tests.rs:1592. Parent baseline audit confirms the untouched regression file and historical baseline failure; no Codex production or Usage code changed.
- final-agent-product-provider_catalog_tests.log was an accidentally unfiltered full-library run caused by optional PowerShell argument splatting. It was interrupted, recorded as INTERRUPTED / NOT_ACCEPTANCE_EVIDENCE with no invented process exit code. No result from that run is counted.
- snapshot-agent-product-provider_catalog_tests.log and snapshot-mcp-provider_query_tests.log: exit 101, UNAVAILABLE / test bodies NOT_RUN due Windows LNK1104 while the old full-Codex test executable was still running. Later build/test operations ran serially after that process ended; final tests above pass.
- verified-mcp-provider_query_tests.log: exit 0 but zero test bodies due an incorrect module filter; NOT_ACCEPTANCE_EVIDENCE. Correct filter is mcp::orchestration_tests::provider_query_tests and its 4/4 result is above.

Scope is restricted to generic Execution staging/finalization, CodeBuddy transport/Fresh Execute/recovery, native fixtures/tests, and capability/catalog projections. Existing three baseline regression files are unchanged; no schema, dependency or lockfile changes. The new execute module and native child fixture are untracked source files intentionally included in review scope. Independent review/freeze is owned by the main session.

## Independent review repair — round 1

Round 0 FULL_SCOPE found one P1: the cross-platform catalog snapshot normalized only canRecover after its shared JSON gained canExecute/activity=true. Repaired only product/provider_catalog_tests.rs to normalize all three with cfg!(windows), and updated the explanatory comment. Production capabilities remain unchanged; the other fixture consumer is Windows-gated and needs no change.

- Preserved original freeze.json byte-for-byte as freeze-round0.json before repair. Both SHA256: B5AEEFF31A70679CFC7CB4210E7291C34350AC0903FDA2297089B7068A58FA1B.
- repair1-catalog.log: native Windows focused catalog module PASS 5/5, exit 0.
- repair1-fmt.log and repair1-diff-check.log: PASS exit 0. Exact cwd/commands/exits appended to validation-results.jsonl.
- Static non-Windows inspection: cfg!(windows) is false, so expected canExecute/activity/canRecover now all match the production false values. This is static evidence only. Linux runtime/build remains UNAVAILABLE without the project Docker runner.
- No unaffected CodeBuddy/Codex rerun; no production changes. Ready for fresh whole-scope freeze and independent re-review; no further edits planned.
