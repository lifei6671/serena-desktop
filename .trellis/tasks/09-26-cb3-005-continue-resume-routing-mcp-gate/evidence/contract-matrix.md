# CB3-005 contract evidence map

This is a requirement-to-test map. Actual execution results and counts belong in delivery.md and command logs; a listed test alone does not mean PASS.

## Existing frozen gates to rerun

| Requirement | Existing evidence test / module |
|---|---|
| explicit / legacy Start; half-field INVALID_PARAMS; strict parser/schema | `mcp::orchestration::dto::tests::start_routing_serde_parser_and_schema_matrix` and `start_legacy_workspace_and_context_validation_regression` |
| general unconfigured; no prompt classification | `start_routing_tests::legacy_general_route_is_required_and_ignores_prompt_classification` |
| Start registered/enabled/health/capability and no side effects | `start_routing_tests::explicit_routing_error_priority_has_no_creation_side_effects` |
| Start frozen retry identity | `start_routing_tests::explicit_retry_and_changed_provider_or_role_use_frozen_v3_identity` |
| CB3-004 actual policy/health/management race boundaries | all eight `mcp::orchestration_tests::start_routing_tests` tests |
| rejected-dispatch fixture stays deterministic | `agent::product::tests::rejected_dispatch_fixture_fails_before_acceptance_without_runtime` plus three existing MCP Start regression tests |
| Continue workspace/taskRole inheritance, no forged identity | Store `continuation_inherits_parent_role_and_rejects_forged_identity`, `continuation_rejects_incomplete_parent_workspace_snapshot_without_fallback` |
| Continue lifecycle/Claim/source revision | Store `continuation_guards_keep_eligibility_claim_and_workspace_contracts`, `continuation_creation_rechecks_provider_opaque_source_revision` |
| work membership and inactive Work | Product `wrong_work_guards_are_side_effect_free_and_cancel_can_converge_inactive_work` |
| disabled Continue/Resume no mutation | TaskManager `disabled_continue_rejects_before_child_creation_or_provider_validation`, `disabled_resume_preserves_execution_and_claim_byte_for_byte` |
| Continue unavailable/capability check | TaskManager `continuation_validation_obeys_health_and_capability_admission` |
| Cancel disabled/unavailable still reaches registered provider | TaskManager `disabled_unavailable_provider_cancel_uses_registration_and_preserves_manual_resolution` |
| Cancel missing registration / capability fail closed | TaskManager `cancel_capability_and_unknown_registration_fail_closed` |
| Resume existing pipeline/replay gate | Product `resume_pending_uses_existing_explicit_pipeline_and_rejects_replay` |
| Remote registry adds no mutation | `providers_remote_registry_has_no_mutation_and_start_accepts_explicit_pair`, registry `fixed_surface` |
| Descriptor hashes / output unchanged | registry `orchestration_fingerprints_cover_each_descriptor_and_ignore_object_key_order` |

Frozen public descriptor hashes (include name, description, input/output schemas and annotations):

- agent_execute: `d9f562430843fcb9ae663cff5b895afef93ab45c23bd29b4af16cf85fd199fa1`
- agent_query: `96bf06b880b5ec4972a42dd83cb1dc7e8f1f8c797f760dbad24c333466d68ae0`

## Required observable state matrix

| Action | Authority | Admission rejection | Successful effect |
|---|---|---|---|
| Continue | source Execution workspace/provider/taskRole | no new Execution, Claim, Runtime or attempt; source unchanged | child freezes source identity |
| ResumePending | same persisted Execution provider/taskRole | execution/claim/dispatch/binding/attempt evidence unchanged | first dispatch of same Execution; no replacement |
| Cancel | persisted provider; registration + canCancel | missing registration/capability rejects | disabled/unavailable does not prevent calling cancel |

Current roleRouting is a discovery snapshot. Changing it never replaces source identity. CodeBuddy future Continue/recover/usage capabilities remain false; this task supplies no real CodeBuddy Session evidence.

## New public-path matrix

All six tests live under `mcp::orchestration_tests::start_routing_tests::continuation_routing_tests` and call the real Broker/Product/Store stack with a deterministic in-process Provider fixture.

| Test | Assertions |
|---|---|
| `continue_inherits_frozen_identity_while_query_reports_current_route` | source and child provider=fixture/taskRole=testing, identical workspace id/root/generation/profile; parent row unchanged; provider receives source ID; current testing route=other; query output passes Ajv; forged provider migration fails with unchanged evidence |
| `continue_admission_rejections_preserve_all_persisted_evidence` | missing registration, disabled, unavailable, canContinue=false, validation Ineligible all reject; complete Execution/Claim/Runtime/attempt/link rows unchanged; no extra execute calls; providers query remains readable after each error; false Continue/recover/tokenUsage projection |
| `resume_rejections_preserve_claim_then_reenable_dispatches_same_identity` | disabled and unavailable preserve all evidence; re-enable restores dispatch of the same ID/provider/taskRole despite changed route; exactly one Execution remains |
| `resume_keeps_runtime_attempt_evidence_and_rejects_replay` | disabled preserves an existing attempt; re-enable cannot bypass attempt gate; no execute call |
| `cancel_remains_registration_only_under_disabled_unavailable_policy` | missing registration and canCancel=false fail closed with stable internal/public errors; disabled+unavailable still invokes registered cancel on the persisted ID and releases pending Claim |
| `continuation_cancel_resume_strict_schema_and_parser_reject_identity_fields` | three valid action inputs; 3 actions × 3 forbidden identity fields rejected by actual parser and actual published schema through Ajv |

Frozen/current example asserted by the first test (fixture evidence, not a real CodeBuddy session):

```json
{
  "sourceExecution": {"provider": "fixture", "taskRole": "testing"},
  "childExecution": {"provider": "fixture", "taskRole": "testing"},
  "providersQuery": {"roleRouting": {"testing": "other"}}
}
```

Error semantics remain stable: Continue registration/capability/validation rejection projects `AGENT_CONTINUE_NOT_ALLOWED`; disabled/unavailable retain their explicit codes. Cancel internal NOT_FOUND/CAPABILITY_UNSUPPORTED continue to project `AGENT_OPERATION_FAILED`. This card changes no public error mapping.
