# Implementation Plan — P2A2-002

1. Add the isolated WorkspacePathResolver module and register it privately.
2. Implement lexical relative-path validation, nearest-existing-parent
   canonicalization, and platform-correct containment checks.
3. Add focused valid, absolute/UNC/parent, nonexistent-target, and Windows
   junction escape tests.
4. Run focused tests, locked library check, scoped formatting, diff checks,
   then a read-only delivery review. Do not migrate callers, commit, or push.
