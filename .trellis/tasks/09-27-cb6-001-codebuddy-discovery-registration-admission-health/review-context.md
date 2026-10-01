# CB6-001 Independent Review Context

## Frozen target

- target identity: `sha256:a5a952678c631d58cfcd612b59839d476b5a7e7ef543eab0f9c6022e41f6c0c5`
- exact per-file hashes: `review-target.json`
- baseline HEAD: `f36bbc3a07fb6d0b7e4e21ad3af024584720077b`
- review mode: independent, read-only, full target coverage

## Contract

Review against CB6-001 in `docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md` and §6.1, §14.1～§14.2.1 of `docs/technical-design-multi-agent-provider-codebuddy-v0.1.md`.

The implementation must remain presence-only: no ACP initialize/session/prompt, Runtime/Windows Job, auto-install/npx, StateStore migration, Continue/Usage/Cancel implementation, UI or remote protocol. All six CodeBuddy capabilities remain false. Missing CodeBuddy remains registered/unavailable and cannot affect Codex.

## Required lenses

- discovery priority, refresh semantics, `%VAR%` expansion and Windows case-insensitive dedupe;
- strict extension filtering and `buddycn` diagnostic-only handling;
- npm wrapper resolution to real executable + argv;
- descriptor/resolved LaunchSpec separation and diagnostic redaction;
- Registry bootstrap/refresh behavior and absence of Runtime/Session/Execution/Claim side effects;
- capability conservatism, public error stability, Codex regression and test completeness;
- scope compliance and Material Contract Difference detection.

## Verification supplied to reviewer

Required Rust gates passed after repairing the first review findings: fmt, clippy, 19 CodeBuddy tests, 1 focused policy health test, 4 provider catalog tests and 29 TaskManager tests. The MCP fixture consumer reached and passed its catalog equality with Registry discovery frozen inside the task-local override, then encountered the repository's Node/npm-dependent contract helper with `program not found`; it is recorded as environment failure, not PASS.

The first frozen target `sha256:d5d10708ed07027ffaafe2b0eeda91ece6792ac2f3eb0a3dc89c48564c3f41db` was blocked for lazy Registry initialization outside the test override (P1) and incomplete Windows path identity (P2). The new target fixes both: Registry initialization occurs inside the override, and Windows path dedupe uses UTF-16 `CompareStringOrdinal` per component while preserving drive-root versus drive-relative identity.

The reviewer must return target identity, `COMPLETE` or `INCOMPLETE` coverage, findings classified P0/P1/P2 with file/line evidence, and verdict `APPROVE` or `BLOCK`.
