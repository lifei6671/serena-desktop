# P2A2-002 WorkspacePathResolver

## Goal

Introduce the internal, Lease-rooted path resolver required for future Source
and Git callers. It derives targets only from `WorkspaceLease.canonical_root`
and a Workspace-relative input.

## Requirements

- Reject Windows and Unix absolute paths, UNC paths, the Workspace root, and
  parent-directory escape attempts.
- Canonicalize the target when it exists; otherwise canonicalize its nearest
  existing parent. In either case, reject targets whose resolved path escapes
  the Lease root through a symlink, junction, or other reparse point.
- Preserve Windows case-insensitive containment semantics.
- Do not rename public `relative_path` or `path` fields, migrate Source/Git
  callers, add filesystem operations, alter MCP schemas, or accept a
  caller-provided root.

## Acceptance Criteria

- [x] A valid relative path resolves below the Lease root.
- [x] Windows/Unix absolute paths, UNC paths, root paths, and parent traversal
  are rejected.
- [x] Existing junction/reparse escapes and nonexistent descendants below an
  escaping parent are rejected.
- [x] A nonexistent target below an in-root existing parent is accepted.
- [x] Focused tests, locked library check, scoped Rust formatting, diff checks,
  and final review pass.

## Verification Evidence

- `cargo test --locked --lib workspace_path` — PASS, 3 passed, including the
  Windows junction test for existing and nonexistent escape targets.
- `cargo check --locked --lib` — PASS. Existing `replace_workspaces` dead-code
  warning remains outside this task's ownership.
- `rustfmt --edition 2024 --check --config skip_children=true
  src/workspace_path.rs src/lib.rs` — PASS.
- `git diff --check` and `git diff --cached --check` — PASS.
