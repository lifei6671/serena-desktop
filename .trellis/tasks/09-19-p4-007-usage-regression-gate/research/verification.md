# P4-007 Verification Evidence — Usage Regression Gate

## Result

`P4-007 PASS`。Full lib 的状态按 Host 规则表述为 **`PASS_WITH_FROZEN_HISTORICAL_BASELINE`**，不是全绿：只保留冻结的三项历史失败。没有发现新的 production regression。

## G01～G17 Gate Matrix

| Gate | Result | 实际覆盖 |
|---|---|---|
| G01 Public strict validation | PASS | `agent::usage::tests` 16；nullable/zero、float/negative/string/overflow、Provider total 不推导。 |
| G02 v9 migration / constraints | PASS | `agent::store::tests` 30；`v9_migrates_real_v8_fixture_without_backfilling_usage`、atomic rollback、constraints、restart。 |
| G03 pinned parser | PASS | `agent::codex::protocol::usage_tests` 11；accepted P4-003 fixture、required breakdown、cache-write absent、context metadata、`USAGE_EVENT_INVALID`。 |
| G04 exact identity publish | PASS | `agent::codex::provider::adapter_tests` 28；exact execution/runtime/thread/turn、safe telemetry、duplicate/out-of-order stateless。 |
| G05 Fresh provenance-zero | PASS | `baseline_intents_require_provenance_and_never_forge_fresh_zero` 与 `fresh_and_warm_projection_are_total_only_and_revision_is_semantic`。 |
| G06 Warm same-epoch delta | PASS | 同一 store test：100→150=50，100→180=80。 |
| G07 restart/epoch unknown | PASS | `unknown_cross_runtime_and_regression_are_fail_safe_and_atomic`：新 runtime 50 保持 unknown/null，epoch record 独立。 |
| G08 no account checkpoint | PASS | 新增 test-only `codex_usage_baseline_never_routes_through_account_usage_read`；另以 production-only `rg` 得到 `NO_PRODUCTION_MATCH`。 |
| G09 same-epoch regression | PASS | `unknown_cross_runtime_and_regression_are_fail_safe_and_atomic` 断言 `USAGE_COUNTER_REGRESSION` 且 public row 不变。 |
| G10 breakdown boundary | PASS | protocol cache-write/context tests + store total-only delta + Product null-total-with-breakdown projection。 |
| G11 terminal grace/freeze | PASS | store 2,000ms inclusive / 2,001ms frozen、identity no-op；adapter 28 覆盖 terminal release-before-drain、late Usage 和 failure isolation；title cleanup test PASS。 |
| G12 runtime teardown freeze | PASS | `freeze_and_runtime_termination_preserve_public_usage_and_isolate_runtimes`。 |
| G13 completeness | PASS | `expired_and_frozen_usage_are_atomic_noops_without_codex_complete`；Product fixture 仍可投影 generic `complete`。 |
| G14 Product projection | PASS | `product::tests::usage_projection_tests` 5：absent unknown/null/revision0、zero、all completeness、history/list/detail、revision isolation、不求 total。 |
| G15 no Usage N+1/private leak | PASS | Product structural test asserts initial `LEFT JOIN execution_usage`、views 不查询 usage/private tables；MCP schema tests deny private fields。 |
| G16 MCP contract | PASS | three descriptor tests；`agent_query=67f707ccd1b8cc2853b1e48160ad6185ec064d4a3e8e2ce60edc8e50e3f7f60c`，`agent_execute=545a7f6f77331ad3a44b22a308f9c3517a897612ffa2a7e8b987644c02b8a63c`。 |
| G17 cleanup title regression | PASS | `title_received_during_cleanup_is_durable_before_next_warm_turn` 1/1。 |

## Focused Rust Commands

All commands below ran from `src-tauri` and exited 0 unless stated otherwise.

| Command | Exit | Total / result |
|---|---:|---|
| `cargo test --locked agent::usage:: --lib -- --nocapture` | 0 | 16 passed |
| `cargo test --locked agent::store::usage_tests --lib -- --nocapture` | 0 | initial 11 passed; after test-only assertion rerun 12 passed |
| `cargo test --locked agent::codex::protocol --lib -- --nocapture` | 0 | 11 passed |
| `cargo test --locked agent::codex::provider::adapter_tests --lib -- --nocapture` | 0 | 28 passed |
| `cargo test --locked agent::telemetry_projector::tests --lib -- --nocapture` | 0 | 3 passed |
| `cargo test --locked product::tests::usage_projection_tests --lib -- --nocapture` | 0 | 5 passed |
| `cargo test --locked agent_output_contract_stable_fields --lib -- --nocapture` | 0 | 1 passed |
| `cargo test --locked agent_query_compact_schema_is_separate_and_execute_retains_existing_contract --lib -- --nocapture` | 0 | 1 passed |
| `cargo test --locked orchestration_fingerprints_cover_each_descriptor_and_ignore_object_key_order --lib -- --nocapture` | 0 | 1 passed |
| `cargo test --locked title_received_during_cleanup_is_durable_before_next_warm_turn --lib -- --nocapture` | 0 | 1 passed |
| `cargo test --locked agent::store::tests --lib -- --nocapture` | 0 | 30 passed |

Focused total is 109 passing test executions after the test-only assertion. The repeated 12-test store run replaces the initial 11-test result; it is not a second independent contract.

## Full-Suite Baseline Comparison

### Product full

`cargo test --locked agent::product::tests --lib -- --nocapture` exited 101 by design of the frozen baseline: **117 total; 111 passed, 5 ignored, 1 failed**.

| Expected frozen failure | Observed signature | Comparison |
|---|---|---|
| `agent::product::tests::orchestration_tests::public_vertical_work_source_start_continue_acceptance_e2e` | `src/agent/product/orchestration_tests.rs:536`, `called Result::unwrap() on an Err value: "WORKSPACE_CONTEXT_REQUIRED"` | MATCH |

### Full lib

Final `cargo test --locked --lib --quiet` ran **1,114 total** and exited 101 only because of the frozen failures. The progress completed through 1,114/1,114 and printed exactly these three failed test names; the extra total versus the prior 1,113 run is the test-only G08 assertion.

| Expected frozen failure | Observed failure signature | Comparison |
|---|---|---|
| `remote::manager::boundary_tests::quick_manual_probe_failure_hides_url_and_retry_restores_ready` | `src/remote/boundary_tests.rs:1065`, `assertion failed: broker.remote.probe().await.is_err()` | MATCH |
| `remote::manager::boundary_tests::metadata_and_401_without_working_handler_cannot_be_ready` | `src/remote/boundary_tests.rs:614`, `called Result::unwrap() on an Err value: Elapsed(())` | MATCH |
| `agent::product::tests::orchestration_tests::public_vertical_work_source_start_continue_acceptance_e2e` | `src/agent/product/orchestration_tests.rs:536`, `called Result::unwrap() on an Err value: "WORKSPACE_CONTEXT_REQUIRED"` | MATCH |

The two remote signatures were also individually captured with `cargo test --locked <test-name> --lib -- --nocapture` (each exit 101, 0 passed / 1 failed); this prevents a count-only baseline comparison.

## Build and Static Checks

| Command | Exit | Result |
|---|---:|---|
| `npm test -- --test-reporter=dot` | 0 | package `test` script ran 121 passed / 0 failed; npm emitted a non-fatal unknown-config warning and Node used its normal reporter. |
| `npm run lint` | 0 | PASS |
| `npm run build` | 0 | PASS; Vite built 2,243 modules |
| `cargo check --locked` | 0 | PASS; six pre-existing dead-code warnings only |
| `cargo fmt --all -- --check` | 0 | PASS from `src-tauri` |
| `git diff --check` | 0 | PASS; existing CRLF conversion warnings only |

An initial `cargo fmt --all -- --check` invoked from repo root exited 1 because there is no root `Cargo.toml`; it was a command-directory error, corrected by the succeeding `src-tauri` command above.

## Contract Source and Change Declaration

- `rg -n --glob '!**/*test*.rs' 'account/usage/read' src-tauri/src` returned `NO_PRODUCTION_MATCH`.
- The Host-provided SHA256 values remain exact for the two authority documents and all seven declared production files. No production file was changed by P4-007.
- P4-007 delivery-owned source change is one **test-only** addition in the pre-existing untracked `src-tauri/src/agent/store/usage_tests.rs`: static assertion that the Codex provider/protocol baseline paths never contain `account/usage/read`. It is an allowed integration/static-contract addition, not a stale-fixture repair.
- P4-007 also owns only `.trellis/tasks/09-19-p4-007-usage-regression-gate/` planning and evidence artifacts. All other dirty files predate this task and are excluded.
- No test fixture expectation was repaired. No production repair, staging, commit, push, UI change, or real account endpoint call occurred.

### Verified SHA256

`implementation-task-breakdown-agent-platform-v0.2-revision003.md` `d86aedbe60ac8701fbcbff06369566d33f282454c6eef5f4aae198db1908132a`
`technical-design-agent-platform-v0.2.md` `42c0f86bb294dc0425c5a8deee5d9f3f36586704218a832e8a9fe89e5f1d9f19`
`protocol.rs` `1273a2c000817e243fc6aede3f6348fe14c66a36ea5b524fd7f5a3f0484951c9`
`provider.rs` `cd579c4cbb39b021a2645def46e807980f2cfc241fd106ad49d8524cd6a35350`
`product.rs` `f1152a469ffe39e42ffe16aa5dcc5bfd81c6f640f6e32e60fc0a8a9a2311dc3e`
`schema_v9.sql` `63f3864da035ad62e913d64f20aa60e985629bdf8047c8cc68bbdc773670896a`
`store/usage.rs` `90f53684f4880aa6ec026534f9a79f8f44f149d94e00a2d53c3a446558821f67`
`usage.rs` `9ffafcd7463323e14dddb089d8dbaa673568f38b185133f1041737655020ef4f`
`mcp/registry.rs` `b26a496540e63ec0876284655e87e4d1c351edb706b8c01c7d774c626b42d0fa`
