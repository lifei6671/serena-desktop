# Delivery review context

Scope: CB3-002 only. Host version hashes verified exactly against user-provided references. User authorized implementation and Trellis artifacts, forbids commits and follow-on tasks. Starting HEAD/status/hashes are in baseline-*; preserve unrelated work. Skill trellis-start and trellis-update-spec are absent from empty project .codex/skills and searched global skill/plugin paths; use current .trellis/workflow.md. Applicable spec packages: single repo, frontend only; no backend index. Read guides cross-layer and code-reuse; UI excluded.

Risk: public MCP protocol, focused Tier 3 review, Standard Mode. Full-scope independent CHILD_AGENT review after code/test freeze; no Host Gate claim. Relevant lenses: strict external input, generated schema/DTO equality, canonical descriptor hashes, existing query compatibility, sanitized error boundary, read-only state/resource lifecycle, registration surface and change provenance.

Requirements -> implementation -> evidence to inspect:
- action-only providers -> empty struct AgentQuery variant + existing parse/validate -> serde/schema strict matrix and transport INVALID_PARAMS
- Product authority -> QueryData directly wraps ProviderCatalogSnapshot + provider_catalog -> exact JSON equality and schema validity
- read-only -> no Runtime/ACP/Session/Execution/Claim/health/policy/binding actions -> traps and before/after snapshots for successful and failed reads
- compatibility -> existing global gate, get/list/observe, agent_execute unchanged -> regression tests, execute frozen hash
- Remote no mutation -> unchanged allowed tool names + provider mutation rejection assertions

Only native Windows checks are intended; Linux NOT_RUN, never WSL. Clippy known blocker usage_tests.rs await_holding_lock is explicitly outside task and must not be fixed. Review uses exact logged command exits/counts, never inferred passes. Evidence and code-hashes.json are supplied by implementer after final runs. Review all delivery-owned hunks, untracked tests and JSON artifacts. For touched pre-existing dirty files compare task baseline copies, not only git HEAD.

Spec-sync decision: existing frozen contract and established orchestration pattern; no new architecture/convention requiring .trellis/spec changes. No task-outside spec edit. No commit/archive/journal mutation; final state remains pending Host independent acceptance.
