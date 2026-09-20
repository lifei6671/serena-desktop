# P6-RG-10C test-only readonly permissions restore

## Goal

Replace test-only readonly permission clearing with restoration of the original fs::Permissions snapshots in the eight approved Windows failure-path tests.

## Requirements

- Only update the eight approved Windows test-only `clippy::permissions_set_readonly_false` occurrences: one in `src-tauri/src/mcp/source_write_atomic_replace.rs` and seven in `src-tauri/src/workspace_registry.rs`.
- Before the first `set_readonly(true)`, preserve the original `fs::Permissions`; clone that snapshot to create each readonly fault-injection permission value.
- After the failure assertion is complete, restore the original snapshot with `fs::set_permissions(path, original_permissions)`; the rename/reorder test must reuse one original snapshot and clone it for both fault-injection phases.
- Do not add lint suppression, `PermissionsExt`, `set_readonly(false)`, production changes, assertion changes, fault-injection changes, persistence/atomic-replace semantic changes, path changes, or cleanup reordering.

## Acceptance Criteria

- [ ] The eight target false-readonly calls are replaced solely by original-permission restoration.
- [ ] The targeted modules' focused tests pass on Windows.
- [ ] `cargo fmt --all -- --check`, `cargo check --locked`, `cargo clippy --locked --all-targets -- -D warnings`, and `git diff --check` pass.
- [ ] A separate read-only P0/P1/P2 review covers the final delivery-owned diff.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
