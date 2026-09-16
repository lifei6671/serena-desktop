# P2A2-001 WorkspaceLease Resolver

## Goal

Validate and complete the internal `workspaceId -> WorkspaceLease` authority
foundation without changing public request schemas or request routing.

## Frozen Boundary

`WorkspaceResolver` accepts only an already validated, nonempty workspace ID.
It looks up the Registry entry, validates and canonicalizes its root, and
returns the frozen `{ workspaceId, canonicalRoot, generation }` snapshot. It
does not parse request payloads and does not read DesktopSelectedWorkspace,
session, Broker ActiveWorkspace, or another fallback.

The public Tool Schema / parameter layer remains responsible for absent,
wrong-type, empty, and whitespace-only `workspaceId` values. Those cases are
reserved for P2A2-004 and must not expand this resolver's input type.

## Acceptance Criteria

- [x] A valid registered ID returns the exact workspace ID, canonical root,
  and generation in a WorkspaceLease.
- [x] A valid but unregistered ID returns `WORKSPACE_NOT_FOUND`.
- [x] A registered missing root returns `WORKSPACE_ROOT_NOT_FOUND` without
  removing or mutating its Registry entry.
- [x] A registered root replaced by a regular file returns
  `WORKSPACE_ROOT_NOT_DIRECTORY`.
- [x] Any currently defined root canonicalization/identity failure is covered
  at the Resolver boundary; no new public error contract is invented.
- [x] Resolver unit tests, locked library check, scoped Rust formatting, diff
  checks, and a final delivery review pass.

## Verification Evidence

- `cargo test --locked --lib workspace_resolver` — PASS, 5 passed.
- `cargo check --locked --lib` — PASS. Existing `replace_workspaces` dead-code
  warning remains outside this task's ownership.
- `rustfmt --edition 2024 --check --config skip_children=true
  src/workspace_resolver.rs` — PASS.
- `git diff --check` and `git diff --cached --check` — PASS.

