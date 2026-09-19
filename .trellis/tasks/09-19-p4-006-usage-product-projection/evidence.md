# P4-006 Evidence — Usage Product Projection

## Scope

- Product `ExecutionView.usage` is always present. No `execution_usage` row yields nullable counters/context and `updatedAt` as null, `unknown`, revision 0.
- Persisted public `UsageSnapshot` maps field-for-field; zero differs from null, persisted total is not recomputed, generic `complete` remains representable.
- `StateStore::product_read` reads Usage in its initial `LEFT JOIN execution_usage` query, validates with P4-001, and fail-closes provider/execution identity mismatch. `views()` neither queries Usage per execution nor accesses Codex private state.
- Usage freshness is only `usageRevision`; Product control/revision legacy alias and activity revision exclude Usage.
- MCP compact list/observe projection plus both `agent_query` and `agent_execute` output schemas declare the stable nullable Usage DTO. The execute schema is strict, requires `usage`, and types `usageRevision` as `uint64`. Updated hashes: agent_query `67f707ccd1b8cc2853b1e48160ad6185ec064d4a3e8e2ce60edc8e50e3f7f60c`; agent_execute `545a7f6f77331ad3a44b22a308f9c3517a897612ffa2a7e8b987644c02b8a63c`.

## Focused behavior evidence

`cargo test --locked product::tests::usage_projection_tests -- --nocapture` — PASS, 5 tests:

1. absent row stable shape; persisted unknown, partial total=123, generic complete, and true zero mapping; Action::List has 5 correctly paired executions.
2. complete field mapping, persisted null total with breakdown, and history projection.
3. provider mismatch and invalid persisted data fail closed / DB constraint rejection.
4. Usage update changes only Usage revision/data, not control/activity revisions.
5. typed camelCase schema, no private Usage identity, and structural LEFT JOIN/no per-view Usage lookup evidence.

`cargo test --locked store::transactions::tests -- --nocapture` — PASS, 34 tests.

`cargo test --locked http_agent_query_compact_views_preserve_revision_persisted_result_and_execute_receipts -- --nocapture` — PASS.

`cargo test --locked orchestration_fingerprints_cover_each_descriptor_and_ignore_object_key_order -- --nocapture` — PASS.

`cargo test --locked agent_output_contract_stable_fields -- --nocapture` — PASS; covers the strict required nullable `agent_execute` Usage contract.

`cargo test --locked agent_query_compact_schema_is_separate_and_execute_retains_existing_contract -- --nocapture` — PASS.

`cargo check --locked` — PASS.

`cargo fmt --all -- --check` — PASS.

`git diff --check` — PASS.

## Broader affected Product test result

`cargo test --locked agent::product::tests -- --nocapture` ran 117 tests: 110 PASS, 5 ignored, 2 FAILED. The two failures are in pre-existing unrelated P4 activity/workspace flows: `persistence_tests::title_received_during_cleanup_is_durable_before_next_warm_turn` and `orchestration_tests::public_vertical_work_source_start_continue_acceptance_e2e`. They do not exercise Usage Product projection; no repair was made outside P4-006 authority.

## N+1 boundary

This task intentionally does not refactor pre-existing per-row reads for thread name, claims, busy state, or runtime attempt. It adds no Usage N+1: Usage is already present on `ProductSnapshot` from the initial `product_read` LEFT JOIN, and the Product `views()` loop only projects `s.usage`.

## Boundary confirmation

No UI changes, no MCP actions/parameters, no Usage writes/recalculation, no Codex private identity reads, no P4-007 Gate, no commit, and no push.

## Host review addendum — Product suite regression attribution

- `public_vertical_work_source_start_continue_acceptance_e2e` still fails at the frozen historical signature: `WORKSPACE_CONTEXT_REQUIRED` from the public transport call. It is not repaired in this task.
- `title_received_during_cleanup_is_durable_before_next_warm_turn` is a Phase 4 **NEW_REGRESSION**. After registering the existing `uint64` AJV format for the P4-006 execute schema in the Product contract helper, its real assertion failed: the title remained `turn-0` instead of `turn-0 cleanup`.
- Root cause was P4-005 terminal Usage drain: cleanup emits `ThreadNameUpdated` on the independent Client event queue after the cleanup RPC, and the Usage-only drain consumed and dropped it. The minimal repair persists only a same-root `ThreadNameUpdated` during the original grace window. It neither changes Execution/Claim terminal ordering nor consumes Usage after the fixed deadline.

Host review verification:

- `cargo test --locked title_received_during_cleanup_is_durable_before_next_warm_turn -- --nocapture` — PASS.
- `cargo test --locked agent::codex::provider::adapter_tests --lib -- --nocapture` — PASS, 28 tests.
- `cargo test --locked agent::store::usage_tests --lib -- --nocapture` — PASS, 11 tests.
- `cargo test --locked product::tests::usage_projection_tests -- --nocapture` — PASS, 5 tests.
- `cargo test --locked agent::product::tests -- --nocapture` — 111 PASS, 5 ignored, 1 failed: only the historical `public_vertical_work_source_start_continue_acceptance_e2e` signature above.
- `cargo check --locked`, `cargo fmt --all -- --check`, and `git diff --check` — PASS.
