# CB2-003 verification

CWD: `E:\wx_lifeilin\github.com\lifei6671\serena-desktop`. Windows native PowerShell; no commit, no CB2-004.

## Contract and changes

- Four Tauri commands registered locally only; typed ProviderId / AgentTaskRole deserialize through existing domain validation. Optional providerId=null clears the route.
- Supervisor operation mutex serializes latest-config read/modify/write. Broker management lock serializes local IPC against other config management operations.
- Shared ProviderAdmissionPolicy write lock spans atomic config::save. Only successful persist publishes Supervisor config and admission settings; failures preserve both in-memory authorities.
- Desktop initializer injects the shared policy before Manager/recovery worker clones are published. Standalone tests may still initialize from a config snapshot through Into.
- replace_config preserves current provider settings so stale whole-config saves cannot overwrite dedicated policy mutations.
- Health refresh reuses Codex discovery and publishes a replacement Registry adapter; old Arc handles and persisted Execution identity remain unchanged. Unknown/unregistered providers return AGENT_PROVIDER_NOT_FOUND; settings retain valid unknown providers.

## Health evidence boundary

Windows production discovery uses managed CLI `--version` / `app-server generate-json-schema`; it never connects an app-server or creates Agent Runtime/Session/Execution. The focused health test injects discovery results at the existing test boundary and asserts real Store execution/runtime/claim tables remain empty, pool remains empty, disabled remains independent of health, old Registry snapshot remains unchanged, and unknown CodeBuddy remains unregistered. This is mocked probe-result evidence, not a real installed CLI acceptance run.

macOS existing discovery uses managed CLI probes and writes ownership rows into runtime_instances (see macos_managed.rs probe_ownership_uses_formal_store_owner_without_business_state). These rows describe CLI probe containment/termination, not Agent Runtime or Session. macOS verify explicitly only exports schema, does not start app-server or send JSON-RPC. Keep this existing ownership safety per section 26's short-lived health probe exception. Do not generalize the Windows/mocked zero-runtime-row assertion to macOS. No macOS native execution was available.

## Commands

| Command | Result | Evidence |
| --- | --- | --- |
| cargo check --manifest-path src-tauri/Cargo.toml --locked | PASS exit 0 | Initial 24.27s; final recheck 3.98s |
| cargo test --manifest-path src-tauri/Cargo.toml --locked provider_policy -- --test-threads=1 | PASS exit 0 | First test snapshot: 8/8, before two added assertions/tests |
| cargo test --manifest-path src-tauri/Cargo.toml --locked --lib provider_policy -- --test-threads=1 | PASS exit 0 | 10/10: 9 new tests + existing startup policy test; before final Manager execute assertion |
| cargo test --manifest-path src-tauri/Cargo.toml --locked --lib -- provider_policy agent::provider:: disabled running_execution_survives_disable continuation_validation_obeys_health startup_reconcile_uses_registered cancel_capability_and_unknown --test-threads=1 | PASS exit 0 | Final 52/52 (0 failed, 0 ignored), 7.67s; includes nine new tests and CB2-002 admission/disable/continue/resume/cancel/drain/reconcile |
| cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check | PASS exit 0 | Final owned source formatted; first check failed only on newly added formatting, repaired using rustfmt on eight owned files |
| git diff --check | PASS exit 0 | Only Git LF/CRLF informational warnings |
| cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings | FAIL exit 1 (baseline blocker only) | src-tauri/src/agent/store/usage_tests.rs:987 holds MutexGuard across awaits at 1012/1016; clippy::await_holding_lock. Unmodified as required; no additional diagnostics |
| Linux validation | NOT_RUN | No project Docker runner found; no WSL used |
| macOS native validation | NOT_RUN | Windows host |

## Scope preservation

Untouched baseline files byte-identical against baseline-files.json. delivery.patch is baseline-relative, excludes pre-existing CB2-002 and two baseline fixes. changes.json records final implementation/test file hashes. Existing pending task artifacts remain preserved. No UI, remote mutation, ACP implementation, routing-contract change, dependency change, or commit.

Delivery-only implementation/test delta: 9 files, +665/-14 lines (normalized baseline comparison). Windows linker emitted informational library creation messages during tests; all tests exited 0. No real Codex CLI health acceptance, macOS native execution, or Linux Docker evidence claimed.

## Independent review repair R1

P1 confirmed: Product ResumePending checked startup backend_error before current dispatch admission. After refresh changed Registry health to Available, the stale startup error still rejected Resume. Removed this duplicate precheck; dispatch still validates registered/enabled/health/capability and retains original failure diagnostics when unavailable. No routing or execution identity change.

Added `provider_policy_refresh_recovers_initially_unavailable_resume`: install initial unavailable backend; create pending execution; call real health refresh with injected discovery; substitute only execution adapter with existing FakeProvider; Product ResumePending succeeds, executes once. It fails with the old stale precheck.

- `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib -- provider_policy unsupported_runtime_actions_preserve unavailable_backend_production_initializer disabled_resume --test-threads=1`: PASS exit 0, 13/13, 4.53s; includes all ten CB2-003 new tests, persisted-disabled startup, unavailable startup recovery/read regression, disabled resume. Platform-excluded filters do not count as executed tests.
- `cargo check --manifest-path src-tauri/Cargo.toml --locked`: PASS exit 0, 2.50s after repair.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`: PASS exit 0 after repair.
- `git diff --check`: PASS exit 0 after repair.
- Refreshed changes.json and delivery.patch: nine files, baseline-relative +665/-14. No unrelated baseline edits overwritten.
- R1 后相同 Clippy 命令重跑：FAIL exit 1，仅既有 usage_tests.rs:987 await_holding_lock（await 1012/1016），无新增诊断；保持不修。

## Final cleanup and spec assessment
After final independent review, the temporary byte snapshot and baseline patch were removed. baseline-summary.json retains baseline/current SHA256 and unchanged results; delivery.patch retains the task-owned baseline-relative diff. Final manifest freshness 9/9 PASS. All untouched baseline files remained byte-identical. No shared spec update is needed: this task implements the existing frozen contract; the stale startup-error correction is documented above. No commit or staging performed.
