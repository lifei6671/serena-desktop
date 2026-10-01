# CB5-001 Implementation Plan

1. Capture current workspace/task baseline and ensure no product files are modified.
2. Read CB5-001 card and design §14.2-§14.2.1 / §28.
3. Implement a small task-local probe harness with deterministic functions for discovery-result parsing and version parsing/fallback.
4. Unit-test found / missing / malformed version fixtures.
5. Run actual Windows probe:
   - where codebuddy / where buddycn
   - inspect shim resolution
   - real-entry --version raw
   - PE metadata + product/package metadata
   - SHA256 EXE/CLI/shim as useful.
6. Produce `verification.md` and `binary.sha256`; include exact commands/results and uncertainty.
7. Freeze evidence hashes and run review. Do not start ACP or touch product code.

## Rollback

If binary/CLI layout cannot be proven or version semantics remain ambiguous enough that no safe candidate can be named, mark CB5-001 BLOCKED/PARTIAL with evidence; do not invent support or modify product discovery.