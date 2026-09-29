# Implementation Plan

1. Add and test provider-neutral config/profile/catalog types and validation.
   - Add serde-defaulted `roleDefaults` with bounded optional strings.
   - Add canonical typed `ExecutionProfile` parsing/serialization.
   - Freeze the resolved profile in Start; update retry and continuation identity tests.
2. Add the Provider configuration-catalog port and local command.
   - Resolve workspace ID through the registry under management authority.
   - Return stable admission errors and prove no Execution/Claim side effects.
3. Implement CodeBuddy discovery/application.
   - Project `session/new` raw models and `thought_level` config options.
   - Extend `DesiredConfiguration` and enforce model acknowledgement before reasoning validation/application.
   - Verify temporary managed runtime shutdown/convergence.
4. Implement Codex discovery/application.
   - Extend compatibility checks and app-server model/list types/pagination.
   - Thread frozen model/effort through fresh and continuation wire parameters.
5. Add settings mutation and additive Product/MCP/frontend wire fields.
   - Preserve existing role routing semantics and fixtures.
   - Keep provider catalog lookup side-effect free and separate.
6. Update AgentPanel interactions and presentation.
   - Four-column role rows, per-provider memory, catalog loading/error/unsupported states, and unavailable saved-value rendering.
   - Preserve custom Select, pending/error rollback, and polling generation protection.
7. Verification.
   - Run focused Rust tests for config, product/store, CodeBuddy, Codex, commands, and MCP.
   - Run relevant Node tests, typecheck/lint/build, Rust `cargo fmt --check`, `cargo check`, `cargo clippy`, and `git diff --check`.
   - Do not use WSL. Linux validation is only through a project Docker Desktop runner; if none is available, report it unavailable.
8. Freeze and review.
   - Capture complete changed-file hashes/diff and actual verification evidence.
   - Dispatch a read-only Trellis check/review covering every partition and integration boundaries.
   - Repair only in-scope P0/P1 findings, then reverify and refresh the review target.
   - Leave all new task changes uncommitted and do not push.
