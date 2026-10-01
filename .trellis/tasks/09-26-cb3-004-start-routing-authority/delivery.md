# CB3-004 Implementation Evidence

Status: Host-confirmed fixture repair and fresh scoped Windows verification complete. The prior review was superseded by the Host finding; repaired-fixture independent review is recorded separately in [review.md](review.md). Host Gate remains pending. No CB3-005, UI, provider-policy mutation, fallback, prompt classification, commit or deployment.

The earlier `DESIGN_BLOCKER.md` is historical. `HOST_RESOLUTION.md` authorizes the implemented management→operation solution; Provider Authority stays in Agent/Product/TaskManager.

## Owned diff

`implementation.diff` compares this delivery to the captured dirty baseline, not HEAD. `implementation-manifest.json` freezes 12 source files (SHA-256 per file); manifest SHA-256 is `e91055b9f0d2d29d249d33cef779e000485d89b3808657ad31df4de7677f12e7`. Delta: **+984 / -61**, including the new routing test file. Prior Host changes are preserved and excluded.

- `agent/product.rs`: creation-authority plumbing, new role errors, and a test-only controlled Registry containing the deterministic rejected-dispatch fake.
- `agent/product/tests.rs`: test-only RejectedDispatchProvider and regression proving failure before acceptance with no Runtime/attempt rows.
- `agent/product/work_adapter.rs`: borrowed management authority and retry through final Start validation.
- `agent/task_manager.rs`: current-config routing/admission under management; release before handoff; reject production snapshot-only creation.
- `agent/store/transactions/product.rs`: frozen provider/role in fresh and retry identity; old default creation helpers limited to test fixtures.
- `serena.rs`: only carry frozen provider/role through existing WorkspaceStartCreation and Store call; no routing or policy mutation added.
- `mcp/mod.rs`, `mcp/orchestration.rs`: local legacy / remote typed intent and existing management reference.
- `agent/product/work_adapter_tests.rs`, `workspace_capability.rs`: necessary creation fixture updates.
- `mcp/orchestration_tests.rs`, new `mcp/start_routing_tests.rs`: register eight focused end-to-end tests.

Paths above are relative to `src-tauri/src/`. Task evidence/baseline/logs are additional task artifacts, not implementation code. Existing task was reused; startup metadata and HOST_RESOLUTION context entries are main-session changes.

## Authority and lock path

Remote DTO `StartRoutingIntent` → orchestration typed mapping → Product Work/context preflight → TaskManager `product_submit_resolved_workspace_start`.

At the final boundary (`task_manager.rs:712`), acquire **Broker.management**, then read current ManagerConfig and Registry/admission facts. Resolve the exact role/provider, then call Supervisor `create_workspace_start`, which acquires **Supervisor.operation**, resolves the current WorkspaceLease, checks remove admission, and synchronously commits the existing Store transaction. Drop management immediately after creation returns (`task_manager.rs:730`), before the owned handoff/dispatch worker.

Official policy changes, agentEnabled saves and health refresh share management. No new production lock or configuration owner was introduced. Existing workspace exclusion tests remain green. Created Execution identity is immutable even if a later policy change causes the existing dispatch admission to reject handoff; this does not compensate or rewrite the committed identity.

## Routing matrix

| Intent / stage | Result |
|---|---|
| Any Start, agentEnabled=false | AGENT_DISABLED |
| Explicit: provider not registered | AGENT_PROVIDER_NOT_FOUND |
| Explicit: provider disabled | AGENT_PROVIDER_DISABLED |
| Explicit: role unconfigured | AGENT_ROLE_NOT_CONFIGURED |
| Explicit: configured provider differs | AGENT_ROLE_PROVIDER_MISMATCH |
| Explicit: health unavailable | AGENT_PROVIDER_UNAVAILABLE |
| Explicit: canExecute=false | AGENT_PROVIDER_CAPABILITY_UNSUPPORTED |
| Explicit: all current facts match | Freeze requested provider + taskRole |
| Legacy: general unconfigured | AGENT_ROLE_NOT_CONFIGURED, before provider lookup |
| Legacy: general configured | Resolve current general provider, then registered→enabled→health→canExecute; freeze General |

Provider errors are preserved specifically at Start; Cancel/Continue error projection is unchanged. No Prompt text influences the selected role. The priority matrix tests deliberately combine bad later-stage facts and assert unchanged Execution/Claim/Runtime rows and zero provider execution calls.

## Identity and race evidence

- Explicit testing-role happy path and exact retry traverse the actual Broker/Product/Store path; only one provider call occurs.
- Same key + changed role or changed provider yields EXECUTION_REQUEST_KEY_CONFLICT when the new pair satisfies current policy. Final Store canonicalization replaces the prior row's provider/role with the newly resolved pair before comparing identity.
- Managed Start preflight retry is only a hint; it cannot return before final Authority. It reads the persisted role for payload comparison, so non-General exact retries remain valid.
- Query→route change, query→provider disable and query→real health refresh all reject before creation. The refresh uses the existing discovery test injection and actually publishes Unavailable without launching a CLI.
- A synchronous hook between final validation and Store commit proves route mutation, provider-enabled mutation, health refresh and agentEnabled save all wait for management. After release, persisted provider/role remain unchanged.
- A request waiting for management observes agentEnabled turned off before acquisition and creates nothing.
- The existing handoff hook proves management is available before provider execution begins.
- Store regressions include `historical_v2_general_retry_is_bounded_and_preserves_hash`, `historical_v2_retry_does_not_bypass_unknown_agent_serialization`, `v3_request_key_conflicts_on_every_frozen_identity_change`, product v2/v1 and pre-C2 compatibility. Execution fixed vectors remain unchanged.

## Actual validation

CWD for every command: `E:\wx_lifeilin\github.com\lifei6671\serena-desktop`. All Cargo evidence below is **native Windows**, never Linux/WSL. MCP, Product and static gates were freshly rerun for the fixture repair; unchanged TaskManager/Store/Execution/Provider and the standalone work_adapter row retain their earlier scoped evidence and are not claimed as new repair runs.

| Actual command | Result / counts | Evidence |
|---|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib mcp:: -- --nocapture` | PASS, exit 0; **303 passed / 0 failed / 4 ignored**, repaired final source, 24.37s | fixture-repair-C.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib agent::product:: -- --nocapture` | PASS; 132 / 0 / 5, repaired final source, 33.10s | fixture-repair-D.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib agent::task_manager:: -- --nocapture` | PASS; 47 / 0 / 1 | agent-task_manager-tests.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib agent::store::transactions:: -- --nocapture` | PASS; 67 / 0 / 0 | agent-store-transactions-tests.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib agent::execution:: -- --nocapture` | PASS; 13 / 0 / 0 | agent-execution-tests.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib agent::provider:: -- --nocapture` | PASS; 29 / 0 / 0 | agent-provider-tests.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib agent::product::tests::work_adapter_tests:: -- --nocapture` | PASS; 16 / 0 / 0, pre-repair source, 5.65s | work-adapter-final-tests.log |
| `cargo check --manifest-path src-tauri/Cargo.toml --locked` | PASS, exit 0, repaired final source | fixture-repair-E-check.log |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | PASS, exit 0, repaired final source | fixture-repair-F-fmt.log |
| `git diff --check` | PASS, exit 0; LF/CRLF notices only | fixture-repair-G-diff.log |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings` | FAIL, exit 101; **only existing** usage_tests.rs:987 await_holding_lock (awaits :1012/:1016) | fixture-repair-H-clippy.log |
| Linux validation | UNAVAILABLE / NOT_RUN: no project Docker Desktop runner found; no substitute used | Parent environment inspection |

The new focused suite first passed 7 tests, then expanded to 8 tests covered by the final full MCP run. Intermediate failures were fixed: a test fixture read `id` instead of `workRunId`; global Provider error projection accidentally affected two Cancel regressions and was narrowed to Start; Clippy requested an equivalent let-chain. After that final conditional-only edit, full MCP, affected work_adapter tests, check/fmt/diff and Clippy were rerun. No tests were weakened or skipped.

## Remaining limits

- Existing Clippy issue is recorded, intentionally not repaired in this task.
- No Linux evidence and no real CodeBuddy/ACP execution is claimed.
- Host independently reviews and reruns its Gate; this report is not Host approval.

## Host-confirmed fixture repair

Host independently reproduced **300 passed / 3 failed / 4 ignored** in MCP. The three existing assertions were correct. The old fixture constructed a real Codex provider, forced its health to Available, and assumed `manager.backend_error` would force execute to fail. Current execute does not honor that assumption; the fixture could discover/run the user's real CLI. Thus the earlier green run was environment-sensitive false confidence, not a deterministic fixture guarantee. No production routing/admission/locking/dispatch behavior was changed to accommodate the tests.

The repair changes only two test surfaces:

1. `new_with_rejected_dispatch_for_test` directly builds `ProviderRegistry::new()` and registers `RejectedDispatchProvider` as Available. It no longer constructs or modifies a real Codex adapter.
2. The cfg(test) fake exposes canExecute=true but `execute` unconditionally returns `TEST_DISPATCH_REJECTED` before acceptance. It has no Store, executable, Runtime or discovery dependency. A new regression uses a panicking acceptance sink, asserts the exact fake error, then exercises Product creation and checks `AGENT_OPERATION_FAILED`, `requestAccepted=true`, one persisted Execution, zero Runtime and zero execution-runtime-attempt rows. The original three MCP assertions are unchanged.

Repair-only baseline/diff: `fixture-repair-baseline/`, `fixture-repair.diff`; **2 files +125 / -6**. `fixture-repair-manifest.json` SHA-256: `d76a00400ed5350e449616bde02ba91ca76c781a79a3169c8c2957eb5cc332e5`. The original task baseline remains intact; the newly touched tests.rs baseline captures its pre-repair Host contents.

Fresh repair commands use the same repository-root cwd and native Windows environment. Each test command below is exactly `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib <filter> -- --nocapture`:

| Gate | Exact filter | Actual result / log |
|---|---|---|
| A1 | `mcp::orchestration_tests::agent_execute_start_resolves_the_work_snapshot_without_active_workspace_authority` | PASS exit 0, 1/0/0; fixture-repair-A1.log |
| A2 | `mcp::orchestration_tests::local_agent_start_resolves_explicit_workspace_without_global_active_workspace` | PASS exit 0, 1/0/0; fixture-repair-A2.log |
| A3 | `mcp::orchestration_tests::local_agent_start_holds_supervisor_operation_mutex_from_lease_to_create` | PASS exit 0, 1/0/0; fixture-repair-A3.log |
| Additional fixture regression | `agent::product::tests::rejected_dispatch_fixture_fails_before_acceptance_without_runtime` | PASS exit 0, 1/0/0; fixture-repair-A4.log |
| B | `mcp::orchestration_tests::start_routing_tests` | PASS exit 0, 8/0/0; fixture-repair-B.log |
| C | `mcp::` | PASS exit 0, 303/0/4; fixture-repair-C.log |
| D | `agent::product::` | PASS exit 0, 132/0/5; fixture-repair-D.log |

E `cargo check --manifest-path src-tauri/Cargo.toml --locked`, F `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`, and G `git diff --check` all passed with exit 0. H `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings` failed with exit 101 **only** for the unchanged `usage_tests.rs:987 await_holding_lock` (awaits at 1012/1016). Logs are fixture-repair-E-check.log through fixture-repair-H-clippy.log. No additional gates or cleanup were performed.
