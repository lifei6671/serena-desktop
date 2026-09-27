# Agent Management Surface Cleanup Implementation Plan

1. Capture current CB4-003-post-Host baseline.
2. Inspect AgentPanel render/data dependencies: distinguish main composer/history DOM from sidebar ProjectTaskNavigation/detail data requirements.
3. Remove only main-page composer/history rendering and dead UI-only state if safe; retain rows/history loading needed by sidebar/details/provider active counts/pending blockers.
4. Preserve current workspace bar.
5. Update tests to assert absence of composer/history while preserving sidebar/detail/provider/role flows.
6. Run focused AgentPanel/App tests, npm test, build, lint, git diff --check.
7. Confirm no Rust/Remote files changed; freeze hashes for Host review.

## Rollback Point

If removing the main list requires removing shared execution state needed by Provider Cards/pending blockers/sidebar details, keep the data path and only remove rendering. Do not redesign task storage or navigation.