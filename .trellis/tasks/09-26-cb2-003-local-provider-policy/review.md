# CB2-003 final independent delivery review — R1

Mode: CHILD_AGENT. Gate: PASSED. Verdict: APPROVED for task-owned change. Coverage: COMPLETE; freshness: FRESH. The initial full-scope review below is inherited; R1 adds affected-path and integration review. All nine final file hashes match changes.json. Only review.md was written by reviewer.

## Findings (fixed)

- File: src-tauri/src/agent/task_manager.rs:769; regression: src-tauri/src/agent/task_manager/tests.rs:1794.
- Issue: P1 initial unavailable backend_error rejected Product ResumePending even after successful health refresh published Available.
- Fix: implementer removed stale early return. Resume now reaches current registered/enabled/health/capability admission in dispatch_with_receipt; pending guard remains after admission; unavailable errors retain existing receipt diagnostics. No new authority or execution identity mutation.
- Resolution: RESOLVED. Inspected complete 46-line new test and FakeProvider execute/acceptance behavior. Test calls actual refresh with injected discovery, retains old startup error, then substitutes execution adapter only. Product resume returns the original execution ID and execute_calls==1; restoring the old early return deterministically fails before fake execution. This proves the repaired integration rather than only registry health.

## Findings (not fixed)

None in task-owned code. No P0/P1/P2 remains. The pre-existing Clippy issue is explicitly excluded by user instruction and remains a verification limitation, not a finding against this change.

## Verification

Evidence reviewed from final verification.md (including R1); no duplicate cargo execution by reviewer.

- TypeCheck: PASS, cargo check --manifest-path src-tauri/Cargo.toml --locked, exit 0 after R1.
- Tests: PASS, original full focused scope 52/52; affected R1 suite 13/13, exit 0. Counts overlap and must not be added as unique total. R1 includes all ten new CB2-003 tests plus relevant unavailable/disabled regressions.
- Format and diff: PASS after R1.
- Lint: FAIL after R1, same cargo clippy --locked --all-targets -- -D warnings command; solely existing usage_tests.rs:987 await_holding_lock (awaits 1012/1016), no new diagnostics. Preserved as explicitly required.
- Native macOS/Linux and real installed CLI acceptance: NOT_RUN. Health fixture is mock discovery plus actual registry/store integration; macOS CLI containment-row boundary accepted on source evidence, not runtime execution.

## Final target SHA256
- `src-tauri/src/agent/product.rs` SHA256 `39fa0bae702bdfc355869c6761ed78ab1e2bc6146ce00c1d8bfa6147dc9cf5de`
- `src-tauri/src/agent/provider/control.rs` SHA256 `20c5cce0f12563a141440e81f9b77b805e6fa7634de7d378976c3c58cdd37167`
- `src-tauri/src/agent/provider/registry.rs` SHA256 `88c97cff13078e32091d4357ebc90f3bda978389ee88a88f6e74d9d711a6b4c1`
- `src-tauri/src/agent/task_manager.rs` SHA256 `c33ab92361278ad84000bcf4633324e37b2e798c3cecfc57c053e40a2c13d682`
- `src-tauri/src/commands.rs` SHA256 `328d8a5f47793e93bfcdfbc38a0b46e07465b69fb344a81f1d3d78e9b3b7185b`
- `src-tauri/src/lib.rs` SHA256 `63d46cbaa9009b13a04481272574448b0c8b9bb14fd9416391b2de896404d2d9`
- `src-tauri/src/serena.rs` SHA256 `aae649c90705fefc73085ef8c992b9858115da875bc0b11f98632b860c820f57`
- `src-tauri/src/provider_policy_tests.rs` SHA256 `74fd37574b00b7a1fb10f4220e7af55fbcd104510b6597764e5ad36d7d252143`
- `src-tauri/src/agent/task_manager/tests.rs` SHA256 `655926819623707dfb3ade3916b0344ff536074fe4988b58e3d3cdce0ace8ec2`

---

## Historical initial review (superseded by R1 verdict above)

# CB2-003 independent delivery review — initial target

Mode: CHILD_AGENT; full-scope Tier 3; coverage COMPLETE; target freshness verified (8/8 SHA256 match changes.json). Gate: BLOCKED. Reviewer changed no implementation files.

## Findings (fixed)

None in reviewer pass; read-only review.

## Findings (not fixed)

### P1 — refreshed Available state does not unblock Product ResumePending

Primary changed location: src-tauri/src/agent/task_manager.rs:477 (registry replacement); affected consumer: src-tauri/src/agent/task_manager.rs:769–775.

Confidence: High. On startup with discovery failure, install_backend_resolution records Some(error) in Manager.backend_error. The new health refresh can successfully discover Codex and publish an Available adapter, but never updates that field. Product Action::ResumePending checks the unchanged manager.backend_error after enabled validation and returns it before dispatch. Therefore a pending Execution remains impossible to resume even after health refresh reports Available; restarting Desktop is necessary. The refreshed registry is used correctly by fresh dispatch, so the inconsistent resume branch is specifically exposed by this new mutation path.

Smallest repair: make this resume admission check consume the current registered health/adapter state while preserving required unavailable diagnostics, without changing frozen execution identity or adding another authority. Add regression for initial unavailable -> successful refresh -> Product resume reaching current admission/dispatch (with injected provider execution evidence). Main session has assigned this repair to implementer.

## Coverage and accepted boundaries

- All delivery.patch owned hunks in product.rs, provider/control.rs, provider/registry.rs, task_manager.rs, commands.rs, lib.rs, serena.rs reviewed; entire new provider_policy_tests.rs reviewed. Existing control.rs baseline excluded except necessary interactions.
- Four Tauri wrappers use existing typed deserialization (ProviderId/AgentTaskRole), null route clearing and legal unknown providers. invoke handler registered; remote registry remains closed and tested for UNKNOWN_TOOL. No frontend, ACP, remote mutation, dependencies or routing contract changes.
- Mutation lock order is Broker management -> Supervisor operation -> runtime config mutex -> admission write lock. Admission read releases its guard before any await; no opposite lock order introduced. Latest config is cloned while locked, config::save validation and atomic replacement precede both memory publications; failure leaves both authorities intact. Whole-config replace preserves current provider settings.
- Desktop passes shared admission owner into Manager initialization and worker clones; restart reads persisted settings. Existing running execution and claim remain unchanged. Focused tests inspect actual DB tables and pool rather than returned values alone.
- Registry refresh clones current snapshot under its mutex, replaces only registered entry and preserves outstanding adapter Arcs. Probe failure changes health without changing enabled policy. Unregistered CodeBuddy remains rejected.
- macOS CLI probe ownership rows are not Agent Runtime/Session creation: inspected existing macos_managed.rs verify -> cli -> create_probe_runtime and termination ownership. It runs --version and app-server generate-json-schema, not app-server JSON-RPC connect or execution/session creation. Section 26 permits short-lived health probes. No finding for retaining those runtime_instances ownership rows; do not claim zero rows on macOS. Focused health test injects discovery results, not real CLI acceptance.
- No new project-wide convention; frozen section 26 already supplies contract. No shared .trellis/spec amendment needed. No template/config generator surface changed.

## Verification

Source: implementer verification.md, read after final result update; no redundant cargo run by reviewer.

- TypeCheck: PASS — cargo check --manifest-path src-tauri/Cargo.toml --locked, exit 0.
- Tests: PASS — combined focused provider policy and CB2-002 regression, 52/52, exit 0 (does not cover P1).
- Format: PASS — cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check.
- Diff: PASS — git diff --check.
- Lint: FAIL — cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings, exit 1, only explicitly excluded baseline src-tauri/src/agent/store/usage_tests.rs:987 await_holding_lock. No new diagnostics; no repair/suppression.
- Linux, macOS native and real CLI acceptance: NOT_RUN; reported limitation, not claimed as pass.

## Reviewed target identity
- `src-tauri/src/agent/product.rs` SHA256 `39fa0bae702bdfc355869c6761ed78ab1e2bc6146ce00c1d8bfa6147dc9cf5de`
- `src-tauri/src/agent/provider/control.rs` SHA256 `20c5cce0f12563a141440e81f9b77b805e6fa7634de7d378976c3c58cdd37167`
- `src-tauri/src/agent/provider/registry.rs` SHA256 `88c97cff13078e32091d4357ebc90f3bda978389ee88a88f6e74d9d711a6b4c1`
- `src-tauri/src/agent/task_manager.rs` SHA256 `2ed8bf466385c6fdcb9024b4e87d58fca6b0c2c9ac688d4ec20a7c1cc269efff`
- `src-tauri/src/commands.rs` SHA256 `328d8a5f47793e93bfcdfbc38a0b46e07465b69fb344a81f1d3d78e9b3b7185b`
- `src-tauri/src/lib.rs` SHA256 `63d46cbaa9009b13a04481272574448b0c8b9bb14fd9416391b2de896404d2d9`
- `src-tauri/src/serena.rs` SHA256 `aae649c90705fefc73085ef8c992b9858115da875bc0b11f98632b860c820f57`
- `src-tauri/src/provider_policy_tests.rs` SHA256 `74fd37574b00b7a1fb10f4220e7af55fbcd104510b6597764e5ad36d7d252143`

