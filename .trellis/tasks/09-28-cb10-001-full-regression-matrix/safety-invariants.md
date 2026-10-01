# Safety Invariants Coverage

权威：technical design §31。以下 19 项只有在映射的实际测试成功执行且测试数非零后才标为 covered。

| # | Invariant | Test/evidence | Result |
|---:|---|---|---|
| 1 | Execution 创建后只属于一个 Provider | `explicit_retry_and_changed_provider_or_role_use_frozen_v3_identity`; `persisted_provider_routes_through_registry_trait_object` | PASS |
| 2 | Continue 不改变 Provider | `continue_inherits_frozen_identity_while_query_reports_current_route` | PASS |
| 3 | Role Routing 变化不迁移已有 Execution | `continue_inherits_frozen_identity_while_query_reports_current_route` | PASS |
| 4 | Provider disabled 不取消已有 Execution | `running_execution_survives_disable_until_provider_converges` | PASS |
| 5 | Provider disabled 不阻止 Cancel / Startup Reconcile | `disabled_unavailable_provider_cancel_uses_registration_and_preserves_manual_resolution`; `disabled_missing_cli_refresh_retains_recovery_authority` | PASS |
| 6 | Provider unavailable 不自动 fallback | `creation_rejects_invalid_provider_id_without_fallback`; `query_then_health_refresh_rejects_start_without_side_effects` | PASS |
| 7 | Runtime 从进程创建的第一个可运行时刻进入受管 Job | `managed_job_handshake_and_failure_cleanup`; `persisted_runtime_ownership_and_recovery_evidence` | PASS |
| 8 | Main PID 消失不是 Runtime termination evidence | `native_tree_main_pid_gone_still_requires_job_zero`; Codex `tree_main_exit_is_not_evidence_and_explicit_termination_is_job_level` | PASS |
| 9 | Runtime termination evidence 属于对应 Provider Runtime | `other_provider_complete_is_rejected`; `direct_reconciliation_rejects_runtime_provider_mismatch` | PASS |
| 10 | ACP Session recovery 不替代旧 Runtime termination evidence | `r2_termination_failure_retains_claim_then_resumes_without_new_runtime`; `recovery_uses_persisted_identity_and_cannot_hide_provider_evidence` | PASS |
| 11 | Provider terminal 不直接授权 Claim release | `staged_terminal_result_requires_original_runtime_evidence_and_exact_values`; `late_terminal_ack_conflicting_identity_or_error_retains_claim` | PASS |
| 12 | terminal + Claim release 原子提交 | `terminal_and_claim_delete_failures_rollback_every_field`; `process_crash_at_each_runtime_finalization_boundary_is_atomic` | PASS |
| 13 | unknown side effects 不 replay | `evidence_and_replay_failure_matrix_fails_closed`; `rt01_unbound_runtime_attempt_restart_is_unknown_without_replay` | PASS |
| 14 | 无可靠 Runtime evidence 保持 unknown + Claim | `missing_runtime_retains_claim_and_orphan_is_recovered`; `os_failure_matrix_keeps_unknown_and_claim` | PASS |
| 15 | Remote MCP 不能修改 Provider enabled / Role Routing | `providers_remote_registry_has_no_mutation_and_start_accepts_explicit_pair`; MCP `fixed_surface` | PASS |
| 16 | Agent Role 是 Routing Policy，不是安全 Capability | `provider_contexts_have_only_the_frozen_execution_id_field`; routing/role projection tests | PASS |
| 17 | CodeBuddy ACP cwd 来自冻结 Execution Workspace | `durable_ordering_early_routing_and_acceptance_without_prompt`; `workspace_rejections_precede_runtime_attempt_and_launch` | PASS |
| 18 | ACP 文件/terminal 代理首版不启用 | `durable_ordering_early_routing_and_acceptance_without_prompt` asserts fs read/write, terminal and elicitation are not true | PASS |
| 19 | Claim release 只依赖 persisted evidence，Provider ID 不能授权 | `release_requires_same_runtime_or_persisted_job_evidence`; `recovery_and_finalization_reject_provider_mismatch_and_retain_claim` | PASS |

All mapped tests executed with nonzero total coverage inside the final R15 full suite and the named cases passed. The earlier B1 failure remains preserved as the evidence that exposed CB6-005's corrupt Claim authority regression before normal invariant classification.
