# CB3-001 implementation evidence

Scope: product.rs DTO/service plus provider_catalog_tests.rs and two JSON fixtures. Registry API unchanged. Existing Product Execution projection, MCP schemas/router, UI, Start routing and provider mutations unchanged. Review and Host Gate pending.

Implementation: provider_catalog(&SupervisorState) reads the current validated ManagerConfig once, then reads the existing Registry. Only Registry descriptors produce providers[]; roleRouting preserves the current five keys including unknown IDs and null. enabled defaults false for a registered provider absent from settings. availableForNewExecution uses agentEnabled && enabled && available health && canExecute. Capabilities are copied field by field. Registry returns providers in ID order. Version is omitted when absent. No generic provider-name branch.

Fixtures tested with include_str! and exact serde_json::Value equality:
- src-tauri/src/agent/product/fixtures/provider_catalog_codex.json: real Codex adapter registered, available, enabled; all six declared capabilities including tokenUsage=false; version is the fixed protocol::VERSION constant, not a host CLI probe.
- src-tauri/src/agent/product/fixtures/provider_catalog_current_policy.json: stable alphabetic ordering, absent version omitted, independent enabled/health/capability facts, unknown current route and null, registered provider without policy disabled, configured unknown provider not falsely listed.

Tests: four focused test functions; availability_matrix covers five independent combinations. Runtime connection trap and immediate lifecycle panics guard against dispatch. durable_snapshot compares all Runtime/Execution/Claim rows with one pre-existing Execution and Claim. Successful and failed read retain config bytes, current policy, Registry Arc/health and pool shutdown flag. Failure is induced by a fake adapter changing its returned descriptor identity after registration, so existing Registry health lookup returns AGENT_PROVIDER_NOT_FOUND. The Product propagates this read error without recovery or mutation.

All commands run in E:\wx_lifeilin\github.com\lifei6671\serena-desktop, native Windows PowerShell. Linux/WSL NOT_RUN. This is Product projection evidence, not a real ACP/Runtime gate.

Initial test attempts: FAIL, exit 1, unresolved crate::paths import in new test fixture. Corrected to crate::config::AppPaths. focused-tests.log and focused-tests-final.log retain both failed attempts. The second invocation observed the prior source while correction was being applied; focused-tests-verified.log records the subsequent 3 PASS / 1 FAIL fixture mismatch. The fixture initially guessed tokenUsage=true and omitted version, but the real adapter declares tokenUsage=false and protocol::VERSION. The corrected fixture faithfully preserves these existing facts; no capability advertisement was changed. Initial fmt failed only on new test formatting and new module ordering, both corrected surgically.

Final command statuses are recorded below after completion. No unrelated blocker repairs. No spec update needed: this implements the existing frozen contract without a new cross-project rule.

Final verification:
- PASS (exit 0): cargo test --locked --manifest-path src-tauri/Cargo.toml provider_catalog_tests -- --nocapture. 4 passed, 0 failed, 1259 filtered out; main target 0 tests. Log focused-tests-pass.log. This supersedes initial import and fixture failures retained above.
- PASS (exit 0): cargo check --locked --manifest-path src-tauri/Cargo.toml. Log cargo-check.log. Production source unchanged since this check; later corrections touched tests/fixtures only.
- PASS (exit 0): cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check. Log fmt.log, final state after fixture correction.
- PASS (exit 0): git diff --check. Log diff-check.log.
- FAIL (exit 1): cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings. Exactly one known out-of-scope error: src-tauri/src/agent/store/usage_tests.rs:987 await_holding_lock, awaits at 1012 and 1016. Log clippy.log. No changes to this blocker.

Code/fixture hashes are frozen in code-hashes.json. Linker informational warnings were emitted by Windows test compilation; focused tests still exited 0.
