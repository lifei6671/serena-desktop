# P2A1-002 Canonical Root Identity Helper

## Goal

Implement a reusable workspace registration root canonicalization and identity comparison helper with stable errors and focused tests.

## Requirements

- Provide a reusable helper in the workspace/config domain that validates a
  candidate root exists and is a directory, then returns its canonical root.
- Map a missing root to `WORKSPACE_ROOT_NOT_FOUND` and a non-directory root to
  `WORKSPACE_ROOT_NOT_DIRECTORY`; do not expose operating-system error text as
  the public error code or create another Workspace error code.
- Provide a root identity comparison helper. It must recognize canonicalized
  aliases as the same root, including `dir/../root` and separator variants;
  on Windows, identity comparison must be case-insensitive without lossy
  path-string normalization.
- An ordinary, non-Git directory is valid. The helper must not require or
  create `.git`, Serena, or CodeGraph metadata, and must not modify its input.
- Keep the helper stateless: no persistence, `ManagerConfig` mutation, root
  rewrite, migration, Registry CRUD, picker, command, UI, path resolver,
  Source I/O, or StateStore work.
- Existing persisted Workspace roots remain byte-for-byte unchanged; callers
  may canonicalize and compare them at call time only.

## Acceptance Criteria

- [x] Missing path returns `WORKSPACE_ROOT_NOT_FOUND`.
- [x] A regular file returns `WORKSPACE_ROOT_NOT_DIRECTORY`.
- [x] A non-Git temporary directory canonicalizes successfully without input
  side effects.
- [x] Same-root and separator aliases compare as one canonical identity.
- [x] On Windows, casing aliases compare as one canonical identity and the
  Windows-specific test executes on the host.
- [x] Focused helper tests, existing config tests, `cargo check --lib`, and
  `git diff --check` have recorded outcomes.

## Notes

- Contract authority is `docs/implementation-task-breakdown-agent-platform-v0.2-revision003.md`
  P2A1-002 and `docs/technical-design-agent-platform-v0.2.md` sections 7.4–7.6,
  50 Workspace stable errors, 51.1, and 54.1.
