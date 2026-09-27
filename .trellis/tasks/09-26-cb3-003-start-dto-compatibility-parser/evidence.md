# CB3-003 verification evidence

Implementation ready for independent review; Host acceptance is separate. Native Windows only, cwd `E:\wx_lifeilin\github.com\lifei6671\serena-desktop`.

## Delivery-owned change

Baseline continuity: `.trellis/tasks/09-26-cb3-003-start-dto/baseline/`; current task planning authorization is under this directory. Old task artifacts were preserved. `delivery-delta.patch` isolates only this delivery from that baseline; `source-hashes.json` freezes full current bytes for review.

- `src-tauri/src/mcp/orchestration/dto.rs`: Start routing intent (`LegacyGeneral` / `Explicit`), domain validation and strict schema pair; parse error mapping and 3 tests.
- `src-tauri/src/mcp/orchestration.rs`: one mechanical `..` in Start match only. No routing, dispatch or policy behavior changed; new intent is not consumed by Product in this task.
- `src-tauri/src/mcp/registry.rs`: only agent_execute frozen hash updated relative to baseline.
- `src-tauri/src/mcp/provider_query_tests.rs`: obsolete Start-rejects-pair assertion updated to accepts-pair; Provider mutation assertion retained.

No Product, TaskManager, Provider implementation, Store, Runtime, UI, dependency, commit, or CB3-004 change. The parser requires JSON only; typed test parses nonexistent work/workspace/provider IDs with no service construction or lookup. No routing policy is consulted.

## Final required verification

All commands below executed from repository root. Exact 37 passing Rust names are in `test-names.json`; no ignored tests in these passing filters.

| Command | Result | Evidence |
|---|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml --lib start_compatibility_tests -- --nocapture` | PASS exit 0; 3 tests; 94 schema/serde/parse/registry cases | focused-final.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib mcp::registry::tests -- --nocapture` | PASS exit 0; 20 tests including descriptor hash/key order | registry-final.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib mcp::orchestration_tests -- --nocapture` | PASS exit 0; 14 tests, including nested 4 provider_query_tests and old workspace/context checks | orchestration-final.log |
| `cargo check --manifest-path src-tauri/Cargo.toml` | PASS exit 0 | cargo-check.log |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | PASS exit 0 | fmt-final.log |
| `git diff --check` | PASS exit 0 | diff-check-final.log |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | FAIL exit 101; only known out-of-scope await_holding_lock at usage_tests.rs:987 (awaits 1012,1016), unchanged | clippy.log |

## Matrix and frozen descriptors

`parse-schema-matrix.json` contains 94 pairs of JSON input and expected acceptance, exercised by direct serde deserialization, pure parse, registry validation and actual published schema compiled by existing Ajv. Includes 5 roles; codex/codebuddy/unregistered/non-ASCII IDs; empty/null/wrong types; ASCII/Unicode/control/terminal newline cases; half pairs; unknown/private routing fields; old context; continue/cancel/resume_pending with one/both/null routing fields. `start-examples.json` gives legacy and explicit requests. Legacy workspace null/missing retains WORKSPACE_CONTEXT_REQUIRED; empty/type errors retain existing INVALID_PARAMS messages; invalid legacy context retains WORK_INVALID_ARGUMENT.

Real descriptor snapshots: `agent-query-descriptor.json`, `agent-execute-descriptor.json`. Hash gate actually passed in Rust; JSON extraction independently agrees:

- agent_query CB3-002 unchanged: `96bf06b880b5ec4972a42dd83cb1dc7e8f1f8c797f760dbad24c333466d68ae0`.
- agent_execute previous: `d889e222c362c1e898a5aabe1897d8e4abb3c8e95962b5d0daf1c352be5f02da`.
- agent_execute current: `d9f562430843fcb9ae663cff5b895afef93ab45c23bd29b4af16cf85fd199fa1`.

Execute schema adds pair constraint only in Start; non-start branches remain unchanged. Fixed hash covers descriptor and object-key order stability in `mcp::registry::tests::orchestration_fingerprints_cover_each_descriptor_and_ignore_object_key_order`.

## Exploratory failures and limits

- `focused.log`: `cargo test --manifest-path src-tauri/Cargo.toml start_compatibility_tests -- --nocapture`, exit 101: 2 pass/1 fail. New test incorrectly expected null workspace to return INVALID_PARAMS. Corrected expectation to existing WORKSPACE_CONTEXT_REQUIRED; production workspace code unchanged. Final 3/3 above supersedes this run.
- `mcp-tests.log`: expanded `cargo test --manifest-path src-tauri/Cargo.toml --lib mcp:: -- --nocapture`, exit 101: 292 pass/3 fail/4 ignored. Executed before corrected workspace expectation/new frozen execute hash was applied. Those two failures are now fixed and retested above. Remaining unrelated baseline assertion `mcp::registry::agent_contract_tests::agent_query_compact_schema_is_separate_and_execute_retains_existing_contract` at registry.rs:2175 expects QueryData anyOf=3, while CB3-002 already has 4. Original baseline file contains exactly the same assertion. It is deliberately unchanged; expanded MCP suite is not claimed green.
- `providers-final.log`: unmatched `mcp::provider_query_tests` filter ran 0 tests; not evidence. Actual provider tests are nested under orchestration_tests and all 4 passed there.
- Initial earlier-task compile errors are retained in old task focused.log; no superseded run is presented as passing.
- No Linux/runtime CodeBuddy acceptance or Host Gate claimed.
