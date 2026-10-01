# CB4-001 Implementation Plan

1. Capture dirty baseline and inspect AgentPanel/navigation/styles/tests.
2. Add minimal local read-only Provider Catalog IPC reusing Product provider_catalog.
3. Add TypeScript catalog DTO and api method.
4. Add provider-neutral presentation helpers/card model.
5. Upgrade navigation/page title to Agent 管理.
6. Insert Agent 接入 cards before existing task composer; preserve task flows.
7. Derive activeExecutions from loaded ExecutionView provider identity; runtime display only from active count.
8. Add DOM tests for Codex-only, two-provider, disabled, unavailable, draining, unknown fallback.
9. Run frontend focused/full tests, build, typecheck/lint; backend focused test + cargo check/fmt/diff-check.
10. Freeze delivery and wait for Host Gate; do not enter CB4-002.

## Rollback point

If cards require changing Runtime/Provider routing semantics or inventing backend runtime evidence, return DESIGN_BLOCKER.