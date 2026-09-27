# CB3-005 delivery evidence

Implementation and required scoped Windows verification complete; independent CHILD_AGENT review **PASSED / COMPLETE / FRESH**, 3/3 source paths, no P0–P3 findings, recorded in `evidence/review.md`. Coordinator's post-review hash check found 0 mismatches. Host Gate remains pending. No Phase 4 / CB4-001, CodeBuddy Session/ACP, provider fallback, policy mutation, Force Unlock, Git commit, or unrelated cleanup.

## Change and reason

The real non-Codex Continue test exposed `agent/store/transactions.rs` lineage validation comparing every historical provider against the literal `codex`. It rejected a valid same-provider continuation with public `AGENT_LINEAGE_CONFLICT`. The minimal repair compares the historical provider against the request's already frozen provider instead. It still rejects cross-provider lineage drift, in the same transaction before insertion or Claim creation.

Continue already inherited workspace identity, provider, taskRole and profile from Store and performed admission/validation before creation. ResumePending already used persisted provider and the existing Claim/dispatch/attempt gate. Cancel already used registration-only lookup and canCancel. Those authorities and the CB3-004 Start lock/race path remain unchanged.

Three delivery-owned source files, relative to `src-tauri/src/`:

| File | Delta / responsibility |
|---|---|
| `agent/store/transactions.rs` | 3 additions / 1 deletion: SQL provider equality and Chinese explanation |
| `mcp/start_routing_tests.rs` | 3-line test-module inclusion; original eight tests unchanged |
| `mcp/continuation_routing_tests.rs` | Six public-path regression tests; deterministic process-free provider |

Complete baseline-relative delta: `evidence/implementation.diff`. It excludes earlier dirty work. Exact-byte source hashes: `evidence/implementation-manifest.json`, SHA256 **4887feb1364b4958d18e46288e6cce230a2c482328ee7c9b22068f152f566773**. Preservation check: 3 source files, **0 hash mismatch / 0 unrelated baseline changes**, HEAD remains `a3ed7962d0cfdcf213de7bcfd02cf48bdf195aef`.

## Contract and state evidence

See `evidence/contract-matrix.md` for requirement-to-test mapping, frozen execution vs current routing example, and Continue/Resume/Cancel state/Claim matrix. Tests compare full Execution, Claim, Runtime, Runtime attempt and Work link rows on rejection. The positive Continue test also verifies unchanged source row and rejects a forged cross-provider child. Parser and actual published schema both reject workspaceId/providerId/taskRole for all three subsequent actions.

The actual registry fingerprint test passes with unchanged descriptor hashes:

- `agent_execute`: `d9f562430843fcb9ae663cff5b895afef93ab45c23bd29b4af16cf85fd199fa1`
- `agent_query`: `96bf06b880b5ec4972a42dd83cb1dc7e8f1f8c797f760dbad24c333466d68ae0`

No production capability value was changed. False canContinue/canRecover/tokenUsage projection is tested. Fixture capabilities prove routing behavior only; they do not claim CodeBuddy implementation or passed ACP evidence.

## Actual validation

All commands run from `E:\wx_lifeilin\github.com\lifei6671\serena-desktop`, **native Windows x86_64-pc-windows-msvc**. Every test command below is `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib <filter> -- --nocapture`. Counts are passed / failed / ignored. Exact commands, exit codes and timings are also in `evidence/validation.json`; each row has its full log in `evidence/`.

| Filter / command | Result | Log |
|---|---|---|
| `continuation_routing_tests` | PASS, 6 / 0 / 0, exit 0 | implement-focused-final.log |
| `mcp::orchestration_tests::start_routing_tests` | PASS, 14 / 0 / 0 (original 8 + new 6), exit 0 | start-routing.log |
| `agent::product::` | PASS, 132 / 0 / 5, exit 0 | product.log |
| `agent::task_manager::` | PASS, 47 / 0 / 1, exit 0 | task-manager.log |
| `agent::store::` | PASS, 124 / 0 / 0, exit 0 | store.log |
| `agent::provider::` | PASS, 29 / 0 / 0, exit 0 | provider.log |
| `agent::execution::` | PASS, 13 / 0 / 0, exit 0 | execution.log |
| `mcp::` (includes orchestration, registry, parser/schema/hash gates) | PASS, 309 / 0 / 4, exit 0 | mcp.log |
| `cargo check --manifest-path src-tauri/Cargo.toml --locked` | PASS, exit 0 | check.log |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | PASS, exit 0 | fmt.log |
| `git diff --check` | PASS, exit 0 | diff-check.log |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings` | FAIL, exit 101; only pre-existing `usage_tests.rs:987 await_holding_lock`, awaits at 1012/1016 | clippy.log |
| Linux validation | UNAVAILABLE / NOT_RUN: no project Docker Desktop runner; no WSL or host-as-Linux substitute | context.md |

The three old MCP Start assertions and deterministic rejected-dispatch fixture regression pass inside the fresh MCP/Product runs. No test was ignored or weakened for this card. Existing ignored runtime tests remain ignored; these results do not claim real CLI/CodeBuddy/ACP runtime acceptance.

Initial focused failures are retained honestly: first run 2 passed / 4 failed found the real provider predicate bug plus three fixture status assumptions; next run 4 passed / 2 failed found incorrect test request/error expectations. After correcting the production predicate and matching existing DTO/error contracts, all six focused tests and all listed final regressions passed. Intermediate evidence is in `implement-initial-focused.md` and `implement-focused.log`; first-run raw stdout remains in the execution transcript.

## Review and scope closure

Independent read-only review covers the complete three-file target, public contract and lifecycle interactions. Its terminal result and reviewed hashes are in `evidence/review.md`; Host independently reviews the actual diff and reruns gates.

Spec review: existing technical design section 10 already states the required frozen-provider semantics. No new generic convention, interface or backend architecture was introduced, and this repository has no backend layer spec to update. The task contract/state matrix records the concrete regression and evidence without editing unrelated frontend specs.

Known limits: the explicitly excluded Clippy issue remains; Linux/real CodeBuddy Session acceptance is not claimed. The Trellis task remains in place for Host review rather than being archived or advanced to Phase 4.
