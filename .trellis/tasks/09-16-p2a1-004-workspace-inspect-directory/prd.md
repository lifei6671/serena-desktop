# P2A1-004 workspace_inspect_directory

## Goal

Implement local read-only workspace directory inspection with canonical-root and basename metadata.

## Requirements

- Provide a local Tauri `workspace_inspect_directory(root: PathBuf)` command backed by a pure, read-only inspection function.
- Accept existing directories, preserve P2A1-002 canonical-root semantics, and return only camelCase `canonicalRoot` and optional `folderBasename` metadata.
- Reject a missing root with `WORKSPACE_ROOT_NOT_FOUND` and a regular file with `WORKSPACE_ROOT_NOT_DIRECTORY`.
- Return access-denied and other I/O failures as failures without introducing a new public `WORKSPACE_*` code or reporting success.
- Treat non-Git directories as valid; inspection must not create files or directories and must not call Serena, CodeGraph, Git, Providers, Registry mutation, or readiness flows.
- Register the command in `lib.rs`; do not alter Remote MCP authority or add dependencies.

## Acceptance Criteria

- [x] Existing and ordinary non-Git directories return the canonical root and basename without filesystem side effects.
- [x] Missing paths and regular files map to the existing stable errors.
- [x] I/O classification is covered through a small production-used seam if host ACL denial is not reliable.
- [x] Serialization exposes exactly `canonicalRoot` and `folderBasename` for the return metadata.
- [x] P2A1-002 and P2A1-003 focused tests remain green, as do `cargo check --lib`, targeted rustfmt, and both diff checks.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
